//! The foldz binary end to end on real libmxl flows, loaded from
//! `SAMPLEZ_TEST_LIBMXL` (default `/opt/mxl/lib/libmxl.so`): a 7.1 pilot is
//! downmixed by three presets and a fourth output reads an unconnected
//! input. Every output block read must be exactly one of what the engine
//! defines: the input through the output's gain matrix, the same after a
//! silenced block with the input fading in, or +0.0 throughout (a silenced
//! input, or a block libmxl zero-filled), and every +0.0 block one the
//! engine counted while the blocks were read. The gain matrices are
//! foldz's own folds, so a few gains of the cited tables are checked
//! against hard-coded values. The ops and API listeners answer, and
//! SIGTERM deletes the output flows.
//!
//! A second run takes IS-05 activations on the NMOS listener: a receiver
//! connected to an absent flow reads it once it appears, disconnected it
//! reads silence, and a scheduled activation fades the flow in from the
//! requested sample, rounded up to a block; a disabled sender's flow is
//! gone and is back once enabled; the resource ids survive a restart.
//!
//! Runs without the test harness. With `SAMPLEZ_REQUIRE_RT=1` the RT
//! thread must run SCHED_FIFO and the process memory must be locked.

#![allow(
    clippy::approx_constant,
    reason = "coefficients are written as printed in their sources, not as 1/sqrt(2)"
)]

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use foldz::config::Config;
use foldz::presets::GainMatrix;
use samplez_mxl::input::{Generation, GroupWait, InputConfig, Outcome, SlotState, Timing};
use samplez_mxl::output::{self, Creation, OutputConfig};
use samplez_mxl::{Instance, MxlApi, time};
use serde_json::{Value, json};

const B: u64 = 48;
/// 20 ms, generous for shared CI runners.
const MARGIN: u64 = 960;
/// The engine's input fade, `engine.rampSamples`.
const RAMP: u32 = 480;
const SECOND: Duration = Duration::from_secs(1);

fn main() {
    let t = Instant::now();
    downmix();
    println!(
        "test downmix_outputs_are_bit_exact_against_the_fold_matrices ... ok ({:.1} s)",
        t.elapsed().as_secs_f64()
    );
    let t = Instant::now();
    nmos();
    println!(
        "test is05_activations_route_detach_and_release_at_their_index ... ok ({:.1} s)",
        t.elapsed().as_secs_f64()
    );
}

fn libmxl() -> PathBuf {
    let path = PathBuf::from(
        std::env::var("SAMPLEZ_TEST_LIBMXL").unwrap_or_else(|_| "/opt/mxl/lib/libmxl.so".into()),
    );
    assert!(path.is_file(), "libmxl not found at {}", path.display());
    path
}

fn api() -> Arc<MxlApi> {
    samplez_mxl::load(&libmxl()).unwrap()
}

/// A fresh MXL domain directory on `/dev/shm`, removed on drop.
struct Domain(PathBuf);

impl Domain {
    fn new() -> Domain {
        let shm = Path::new("/dev/shm");
        let base = if shm.is_dir() {
            shm.to_owned()
        } else {
            std::env::temp_dir()
        };
        let dir = base.join(format!("foldz-e2e-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        // A 1 s history, ring of 2 s: room to read a second behind.
        std::fs::write(
            dir.join("options.json"),
            r#"{"urn:x-mxl:option:history_duration/v1.0": 1000000000}"#,
        )
        .unwrap();
        Domain(dir)
    }

    fn has_flow(&self, id: &str) -> bool {
        self.0.join(format!("{id}.mxl-flow")).exists()
    }
}

impl Drop for Domain {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A random version 4 UUID, which BCP-007-03's MXL id pattern admits.
fn new_id() -> String {
    let r = RandomState::new();
    let x = (u128::from(r.hash_one(1u8)) << 64) | u128::from(r.hash_one(2u8));
    let x = (x & !(0xf << 76) & !(0x3 << 62)) | (0x4 << 76) | (0x2 << 62);
    let h = format!("{x:032x}");
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// The pilot: channel `ch` is a sine of `1000 + 250 ch` Hz, exact per index.
fn pilot(ch: usize, idx: u64) -> f32 {
    static SINE: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
    let sine = SINE.get_or_init(|| {
        (0..48_000)
            .map(|phase| (2.0 * std::f64::consts::PI * phase as f64 / 48_000.0).sin() as f32)
            .collect()
    });
    let f = 1000 + 250 * ch as u64;
    sine[((f * (idx % 48_000)) % 48_000) as usize]
}

/// A pilot producer committing block `k` once libmxl's clock passes its
/// end. Dropping it stops the thread and deletes the flow.
struct Pilot {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Pilot {
    fn start(domain: &Path, id: &str, channels: u32) -> Pilot {
        let api = api();
        let instance = Instance::new(&api, domain).unwrap();
        let cfg = OutputConfig {
            flow_id: id,
            label: "pilot",
            group: "pilot",
            role: "Audio",
            channels,
            block: B,
            batch: B,
        };
        let Creation::Created { mut writer, .. } = output::create(&instance, &cfg).unwrap() else {
            panic!("pilot flow {id} not created");
        };
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            let n = B as usize;
            let mut data = vec![0.0f32; channels as usize * n];
            let mut next: Option<u64> = None;
            while !flag.load(Ordering::Relaxed) {
                let idx = time::timestamp_to_index(&api, time::now(&api));
                if let Some(due) = (idx / B).checked_sub(1) {
                    for k in next.unwrap_or(due)..=due {
                        for (ch, out) in data.chunks_exact_mut(n).enumerate() {
                            for (i, v) in out.iter_mut().enumerate() {
                                *v = pilot(ch, k * B + i as u64);
                            }
                        }
                        writer.write_block(k, &data);
                    }
                    next = Some(due + 1);
                }
                let k = next.unwrap_or(0);
                time::sleep_until(&api, time::index_to_timestamp(&api, (k + 1) * B));
            }
            drop(writer);
            drop(instance);
        });
        Pilot {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Pilot {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            t.join().unwrap();
        }
    }
}

/// The foldz process; killed on drop if still running.
struct Foldz {
    child: Child,
    api: SocketAddr,
    ops: SocketAddr,
    nmos: SocketAddr,
}

impl Foldz {
    fn start(config: &Path) -> Foldz {
        let mut child = Command::new(env!("CARGO_BIN_EXE_foldz"))
            .arg("--config")
            .arg(config)
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let lines = BufReader::new(child.stderr.take().unwrap()).lines();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in lines.map_while(Result::ok) {
                eprintln!("  | {line}");
                let _ = tx.send(line);
            }
        });
        let addr = |rx: &Receiver<String>, name: &str| loop {
            let line = rx.recv_timeout(10 * SECOND).expect("foldz start-up line");
            if let Some(a) = line.strip_prefix(&format!("foldz: {name} listener on ")) {
                break a.parse().unwrap();
            }
        };
        let api = addr(&rx, "api");
        let ops = addr(&rx, "ops");
        let nmos = addr(&rx, "nmos");
        Foldz {
            child,
            api,
            ops,
            nmos,
        }
    }

    /// Sends SIGTERM and returns whether the process exited with success.
    fn terminate(mut self) -> bool {
        // SAFETY: kill with a live child pid and a valid signal.
        assert_eq!(
            unsafe { libc::kill(self.child.id() as i32, libc::SIGTERM) },
            0
        );
        self.child.wait().unwrap().success()
    }
}

impl Drop for Foldz {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A plain HTTP/1.1 GET: the status code and the body as JSON, or as a
/// string when it is not JSON.
fn get(addr: SocketAddr, path: &str) -> (u16, Value) {
    request(addr, "GET", path, None)
}

/// A plain HTTP/1.1 request with an optional JSON body.
fn request(addr: SocketAddr, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(5 * SECOND)).unwrap();
    let body = body.map(Value::to_string).unwrap_or_default();
    write!(
        s,
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut text = String::new();
    s.read_to_string(&mut text).unwrap();
    let (head, body) = text.split_once("\r\n\r\n").unwrap();
    let code = head.split(' ').nth(1).unwrap().parse().unwrap();
    let body = serde_json::from_str(body).unwrap_or_else(|_| Value::String(body.to_owned()));
    (code, body)
}

fn wait_for(timeout: Duration, what: &str, mut f: impl FnMut() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(t.elapsed() < timeout, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Reads one output flow block by block, ascending.
struct Reader {
    generation: Generation,
    state: SlotState,
    buf: Vec<f32>,
}

impl Reader {
    fn new(domain: &Path, flow_id: &str, channels: usize) -> Reader {
        let mut generation = Generation::new(&api(), domain, 1).unwrap();
        let cfg = InputConfig {
            flow_id,
            channels: channels as u32,
            flow_channels: None,
            offset: 0,
        };
        let timing = Timing {
            block: B,
            margin: 0,
            max_catchup_blocks: 0,
            resync_budget: 0,
        };
        generation.attach(0, &cfg, &timing).unwrap();
        Reader {
            generation,
            state: SlotState::default(),
            buf: vec![0.0; channels * B as usize],
        }
    }

    /// Blocks `ks`, planar per block.
    fn read(&mut self, ks: std::ops::Range<u64>) -> Vec<f32> {
        let input = self.generation.input(0).unwrap();
        let mut out = Vec::with_capacity(ks.clone().count() * self.buf.len());
        for k in ks {
            let o = input.read(&mut self.state, k, &GroupWait::Skipped, &mut self.buf);
            assert_eq!(o, Outcome::Copy, "block {k}");
            out.extend_from_slice(&self.buf);
        }
        out
    }
}

/// The engine's input fade at `pos` samples into it, as samplez-dsp
/// computes a ramp from 0 to 1: `0 + pos * (1 / len)`.
fn fade(pos: u64) -> f32 {
    if pos < u64::from(RAMP) {
        0.0 + pos as f32 * ((1.0 - 0.0) / RAMP as f32)
    } else {
        1.0
    }
}

/// Output channel `j` at `idx`: the gains of `m` over the pilot, summed in
/// ascending input order from +0.0 with a multiply and an add per nonzero
/// gain, the input faded in from `fade_start` when given.
fn expected(m: &GainMatrix, j: usize, idx: u64, fade_start: Option<u64>) -> f32 {
    let f = fade_start.map_or(1.0, |s| fade(idx - s));
    let mut acc = 0.0f32;
    for i in 0..m.inputs() {
        let g = m.gain(j, i);
        if g != 0.0 {
            acc += g * (pilot(i, idx) * f);
        }
    }
    acc
}

fn block_matches(m: &GainMatrix, k: u64, out: &[f32], fade_start: Option<u64>) -> bool {
    let b = B as usize;
    (0..m.outputs()).all(|j| {
        (0..b).all(|n| {
            let idx = k * B + n as u64;
            let (got, want) = (out[j * b + n], expected(m, j, idx, fade_start));
            if want == 0.0 {
                got.to_bits() == 0
            } else {
                got == want
            }
        })
    })
}

#[derive(Debug, Default)]
struct Counts {
    clean: u64,
    fading: u64,
    /// All-zero blocks followed by a fade-in: the input was silenced.
    silenced: u64,
    /// All-zero blocks followed by a block without a fade: the engine
    /// skipped them and libmxl zero-filled them.
    skipped: u64,
    /// All-zero blocks not followed by a block yet.
    zero_run: u64,
}

impl Counts {
    fn total(&self) -> u64 {
        self.clean + self.fading + self.silenced + self.skipped + self.zero_run
    }
}

/// Checks blocks `ks` of every output: each block of a connected output is
/// all +0.0, the matrix over the pilot, or that with the input fading in
/// from the end of the last all-zero block, carried in `fade_start` from
/// one range to the next. The outputs share one input, so a silenced input
/// silences all of them in the same block.
fn check(
    readers: &mut [(Reader, Option<&GainMatrix>)],
    ks: std::ops::Range<u64>,
    fade_start: &mut Option<u64>,
    counts: &mut Counts,
) {
    // Read first, checked after: checking is slower than the blocks age
    // out of the ring.
    let blocks: Vec<Vec<f32>> = readers
        .iter_mut()
        .map(|(r, _)| r.read(ks.clone()))
        .collect();
    let count = ks.clone().count();
    for (n, k) in ks.enumerate() {
        let mut zero = None;
        for (o, (_, matrix)) in readers.iter().enumerate() {
            let size = blocks[o].len() / count;
            let out = &blocks[o][n * size..(n + 1) * size];
            let all_zero = out.iter().all(|v| v.to_bits() == 0);
            let Some(m) = matrix else {
                assert!(all_zero, "output {o} of an unconnected input, block {k}");
                continue;
            };
            assert!(
                zero.is_none_or(|z| z == all_zero),
                "block {k}: output {o} disagrees on silence"
            );
            zero = Some(all_zero);
            if all_zero {
                continue;
            }
            let faded = fade_start.is_some_and(|s| k * B < s + u64::from(RAMP))
                && block_matches(m, k, out, *fade_start);
            assert!(
                faded || block_matches(m, k, out, None),
                "output {o} block {k}: neither +0.0, nor the matrix over the pilot, nor that \
                 fading in from {fade_start:?}"
            );
            if o == 0 {
                let run = std::mem::take(&mut counts.zero_run);
                if faded {
                    counts.fading += 1;
                    counts.silenced += run;
                } else {
                    counts.clean += 1;
                    counts.skipped += run;
                }
            }
        }
        match zero {
            Some(true) => {
                counts.zero_run += 1;
                *fade_start = Some((k + 1) * B);
            }
            _ => {
                if fade_start.is_some_and(|s| k * B >= s + u64::from(RAMP)) {
                    *fade_start = None;
                }
            }
        }
    }
}

/// The engine counters of +0.0 output blocks: samples skipped behind an
/// overrun, which libmxl zero-fills, and silenced input blocks.
#[derive(Debug, Clone, Copy)]
struct Dropouts {
    skipped_samples: u64,
    silenced_blocks: u64,
}

fn dropouts(foldz: &Foldz) -> Dropouts {
    let (code, status) = get(foldz.api, "/api/v1/status");
    assert_eq!(code, 200, "{status}");
    let c = &status["counters"];
    Dropouts {
        skipped_samples: c["overrunSkippedSamples"].as_u64().unwrap(),
        silenced_blocks: c["inputSilencedBlocks"].as_u64().unwrap(),
    }
}

/// Gains of the outputs [`downmix`] configures, from the cited tables
/// rather than from [`foldz::presets::fold`], which the bit-exact check
/// takes them from.
fn cited_gains(folds: &[foldz::presets::Fold]) {
    let db = |d: f64| 10f64.powf(d / 20.0);
    // Input channels L R C LFE Lss Rss Lrs Rrs; (output, output channel,
    // input channel, gain).
    let cited = [
        // BS.775-4 Table 2, 2/0: C into L and R at 0.7071; the output's
        // gainDb -3 scales every gain.
        (0, 0, 2, 0.7071 * db(-3.0)),
        (0, 1, 2, 0.7071 * db(-3.0)),
        (0, 0, 0, db(-3.0)),
        (0, 0, 1, 0.0),
        // lfe include at -10 dB, then gainDb.
        (0, 0, 3, db(-10.0) * db(-3.0)),
        (0, 1, 3, db(-10.0) * db(-3.0)),
        // TS 103 190-1 Table 219, 3/4/0.x: Lss into Ls at 0.707, then
        // BS.775-4 Ls into L at 0.7071.
        (0, 0, 4, 0.707 * 0.7071 * db(-3.0)),
        (0, 1, 4, 0.0),
        // TS 103 190-1 Table 219, 3/4/0.x: Ls = 0.707 Lss + 0.707 Lrs.
        (1, 4, 4, 0.707),
        (1, 4, 6, 0.707),
        (1, 5, 7, 0.707),
        (1, 2, 2, 1.0),
        (1, 3, 3, 1.0),
        // select by label.
        (2, 0, 2, 1.0),
    ];
    for (o, j, i, want) in cited {
        let got = f64::from(folds[o].matrix.gain(j, i));
        assert!(
            (got - want).abs() <= want.abs() * 1e-6,
            "output {o} channel {j} from input channel {i}: {got}, cited {want}"
        );
    }
}

fn downmix() {
    let domain = Domain::new();
    let (src, stereo, five_one, centre, idle) = (new_id(), new_id(), new_id(), new_id(), new_id());
    let _pilot = Pilot::start(&domain.0, &src, 8);
    let config = json!({
        "mxl": { "libraryPath": libmxl() },
        "engine": { "readerMarginSamples": MARGIN, "rampSamples": RAMP },
        "rt": { "rtCpu": "none" },
        "listeners": {
            "api": { "bind": "127.0.0.1", "port": 0 },
            "ops": { "bind": "127.0.0.1", "port": 0 },
            "nmos": { "bind": "127.0.0.1", "port": 0 }
        },
        "domains": [{ "name": "d", "path": domain.0 }],
        "inputs": [
            { "name": "main", "channels": 8, "flow": src,
              "layout": ["L", "R", "C", "LFE", "Lss", "Rss", "Lrs", "Rrs"] },
            { "name": "spare", "channels": 2, "layout": ["L", "R"] }
        ],
        "outputs": [
            { "name": "stereo", "input": "main", "flows": [stereo], "layout": ["L", "R"],
              "preset": "ac4-bs775-7_1-to-2_0", "lfe": { "include": { "gainDb": -10 } },
              "gainDb": -3 },
            { "name": "five-one", "input": "main", "flows": [five_one],
              "layout": ["L", "R", "C", "LFE", "Ls", "Rs"], "preset": "ac4-7_1-to-5_1" },
            { "name": "centre", "input": "main", "flows": [centre], "layout": ["C"],
              "preset": "select" },
            { "name": "idle", "input": "spare", "flows": [idle], "layout": ["L", "R"],
              "preset": "select" }
        ]
    });
    let file = domain.0.join("foldz.json");
    std::fs::write(&file, config.to_string()).unwrap();
    let (parsed, folds) = Config::load(&file).unwrap();
    assert_eq!(parsed.outputs.len(), 4);
    cited_gains(&folds);

    let foldz = Foldz::start(&file);
    let outputs = [&stereo, &five_one, &centre, &idle];
    wait_for(10 * SECOND, "the output flows", || {
        outputs.iter().all(|id| domain.has_flow(id))
    });

    let (code, status) = get(foldz.api, "/api/v1/status");
    assert_eq!(code, 200, "{status}");
    if std::env::var("SAMPLEZ_REQUIRE_RT").as_deref() == Ok("1") {
        for step in status["rt"].as_array().unwrap() {
            if step["name"] == "sched" || step["name"] == "mlock" {
                assert_eq!(step["state"], "active", "{step}");
            }
        }
    }
    wait_for(10 * SECOND, "the input attached", || {
        get(foldz.api, "/api/v1/status").1["inputs"][0]["state"] == "attached"
    });
    // Past the fade-in at attach.
    std::thread::sleep(SECOND);

    let api = api();
    let now_k = || time::timestamp_to_index(&api, time::now(&api)) / B;
    let mut readers: Vec<(Reader, Option<&GainMatrix>)> = outputs
        .iter()
        .zip(&folds)
        .enumerate()
        .map(|(o, (id, f))| {
            let m = &f.matrix;
            (
                Reader::new(&domain.0, id, m.outputs()),
                (o < 3).then_some(m),
            )
        })
        .collect();
    let mut counts = Counts::default();
    let mut fade_start = None;
    // The engine processes a block only once libmxl's clock has passed its
    // end, so the window starts at a block it had not processed when the
    // counters were read.
    let before = dropouts(&foldz);
    let mut k = now_k();
    // Blocks committed for at least 100 ms, so every output has them.
    let committed = || now_k() - (MARGIN + 4800) / B;
    // Blocks leave the readable history 1 s after they were written, so
    // each round reads what was written while the last one was checked.
    while counts.total() < 3_000 {
        std::thread::sleep(Duration::from_millis(300));
        let end = committed();
        check(&mut readers, k..end, &mut fade_start, &mut counts);
        k = end;
    }
    // A run of +0.0 blocks at the end is read on until a block ends it,
    // which tells a silenced input from a skip.
    let t = Instant::now();
    while counts.zero_run > 0 {
        assert!(
            t.elapsed() < 5 * SECOND,
            "+0.0 blocks up to {k}: {counts:?}"
        );
        std::thread::sleep(Duration::from_millis(100));
        let end = committed();
        check(&mut readers, k..end, &mut fade_start, &mut counts);
        k = end;
    }
    // Every block read was committed, so processed, before this second
    // read: every +0.0 block in the window is one the engine counted
    // between the two. Blocks the engine processed outside the window
    // can only add to the counts.
    let after = dropouts(&foldz);
    eprintln!("  blocks checked: {counts:?}, counted {before:?} -> {after:?}");
    assert!(
        after.skipped_samples - before.skipped_samples >= counts.skipped * B,
        "{counts:?}: {before:?} -> {after:?}"
    );
    assert!(
        after.silenced_blocks - before.silenced_blocks >= counts.silenced,
        "{counts:?}: {before:?} -> {after:?}"
    );
    // The counters account for every +0.0 block; most blocks still carry
    // the matrix, so the bit-exact check covered it.
    assert!(counts.clean * 10 > counts.total() * 9, "{counts:?}");

    let (code, status) = get(foldz.api, "/api/v1/status");
    assert_eq!(code, 200);
    assert_eq!(status["inputs"][1]["state"], "unconnected", "{status}");
    for o in status["outputs"].as_array().unwrap() {
        assert_eq!(o["realised"], true, "{o}");
    }
    let (code, downmix) = get(foldz.api, "/api/v1/downmix");
    assert_eq!(code, 200);
    assert_eq!(downmix["outputs"][0]["preset"], "ac4-bs775-7_1-to-2_0");
    for path in ["/healthz", "/readyz"] {
        let (code, body) = get(foldz.ops, path);
        assert_eq!(code, 200, "{path}: {body}");
        assert_eq!(body["ok"], true, "{path}: {body}");
    }
    let (code, metrics) = get(foldz.ops, "/metrics");
    assert_eq!(code, 200);
    let metrics = metrics.as_str().unwrap();
    let metric = |series: &str| -> u64 {
        metrics
            .lines()
            .find_map(|l| l.strip_prefix(series)?.strip_prefix(' '))
            .unwrap_or_else(|| panic!("no {series} in\n{metrics}"))
            .parse()
            .unwrap()
    };
    let heartbeat = metric("samplez_rt_heartbeat_blocks_total");
    assert!(heartbeat > 1_000, "{heartbeat}");
    for series in [
        "samplez_wake_jitter_max_nanoseconds",
        "samplez_process_time_max_nanoseconds",
    ] {
        assert!(metric(series) > 0, "{series}");
    }
    // Per input, for the connected one only.
    let copied = metric(r#"samplez_input_reads_total{input="main",outcome="copied"}"#);
    assert!(
        copied > 1_000 && copied <= heartbeat,
        "{copied} {heartbeat}"
    );
    metric(r#"samplez_input_reads_total{input="main",outcome="late"}"#);
    metric(r#"samplez_input_validity_probes_total{input="main"}"#);
    assert!(!metrics.contains(r#"input="spare""#), "{metrics}");

    assert!(foldz.terminate(), "foldz exits 0 on SIGTERM");
    for id in outputs {
        assert!(
            !domain.has_flow(id),
            "the released writer deletes flow {id}"
        );
    }
}

/// A TCP port free on 127.0.0.1 a moment ago: the NMOS listener's port has
/// to be known before foldz starts, to advertise it.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// The id of the one resource of `kind` (`senders`, `receivers`) labelled
/// `label`.
fn resource_id(foldz: &Foldz, kind: &str, label: &str) -> String {
    let (code, list) = get(foldz.nmos, &format!("/x-nmos/node/v1.3/{kind}/"));
    assert_eq!(code, 200, "{list}");
    let found: Vec<_> = list
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["label"] == label)
        .collect();
    assert_eq!(found.len(), 1, "{list}");
    found[0]["id"].as_str().unwrap().to_owned()
}

/// PATCHes `staged` of `/single/<kind>/<id>` and returns the response.
fn stage(foldz: &Foldz, kind: &str, id: &str, staged: Value) -> Value {
    let path = format!("/x-nmos/connection/v1.2/single/{kind}/{id}/staged");
    let (code, body) = request(foldz.nmos, "PATCH", &path, Some(&staged));
    assert!(code == 200 || code == 202, "{code} {body}");
    body
}

fn input_status(foldz: &Foldz) -> Value {
    let (code, status) = get(foldz.api, "/api/v1/status");
    assert_eq!(code, 200, "{status}");
    status["inputs"][0].clone()
}

fn immediate() -> Value {
    json!({ "mode": "activate_immediate" })
}

fn nmos() {
    let domain = Domain::new();
    let domain_id = new_id();
    std::fs::write(
        domain.0.join("domain_def.json"),
        json!({ "id": domain_id, "label": "e2e", "description": "", "tags": {} }).to_string(),
    )
    .unwrap();
    let (src, out) = (new_id(), new_id());
    let config = json!({
        "mxl": { "libraryPath": libmxl() },
        "engine": { "readerMarginSamples": MARGIN, "rampSamples": RAMP },
        "rt": { "rtCpu": "none" },
        "listeners": {
            "api": { "bind": "127.0.0.1", "port": 0 },
            "ops": { "bind": "127.0.0.1", "port": 0 },
            "nmos": { "bind": "127.0.0.1", "port": free_port() }
        },
        "nmos": { "enabled": true, "seedName": "e2e", "advertiseHost": "127.0.0.1" },
        "domains": [{ "name": "d", "path": domain.0 }],
        "inputs": [{ "name": "main", "channels": 2, "layout": ["L", "R"] }],
        "outputs": [{ "name": "out", "flows": [out], "layout": ["L", "R"], "preset": "select" }]
    });
    let file = domain.0.join("foldz.json");
    std::fs::write(&file, config.to_string()).unwrap();

    let foldz = Foldz::start(&file);
    wait_for(10 * SECOND, "the output flow", || domain.has_flow(&out));
    let receiver = resource_id(&foldz, "receivers", "main");
    let sender = resource_id(&foldz, "senders", "out");
    let node = get(foldz.nmos, "/x-nmos/node/v1.3/self").1["id"].clone();
    assert_eq!(input_status(&foldz)["state"], "unconnected");

    // Connected to a flow nobody writes yet: accepted, read once it is.
    let connect = |activation: Value| {
        json!({
            "master_enable": true,
            "transport_params": [{ "mxl_flow_id": src, "mxl_domain_id": domain_id }],
            "activation": activation,
        })
    };
    stage(&foldz, "receivers", &receiver, connect(immediate()));
    wait_for(10 * SECOND, "the input on the absent flow", || {
        input_status(&foldz)["flow"] == src.as_str()
    });
    let _pilot = Pilot::start(&domain.0, &src, 2);

    let api = api();
    let now_k = || time::timestamp_to_index(&api, time::now(&api)) / B;
    // Blocks committed for at least 100 ms.
    let committed = || now_k() - (MARGIN + 4800) / B;
    // A reader reads ascending only, so each read takes a new one.
    let read = |ks: std::ops::Range<u64>| Reader::new(&domain.0, &out, 2).read(ks);
    let b = B as usize;
    let is_pilot = |k: u64, block: &[f32]| {
        (0..2).all(|ch| (0..b).all(|n| block[ch * b + n] == pilot(ch, k * B + n as u64)))
    };
    let is_silent = |block: &[f32]| block.iter().all(|v| v.to_bits() == 0);
    wait_for(10 * SECOND, "the pilot at the output", || {
        let k = committed();
        is_pilot(k, &read(k..k + 1))
    });

    // Disconnected: silence, and the input reads nothing.
    stage(
        &foldz,
        "receivers",
        &receiver,
        json!({ "master_enable": false, "activation": immediate() }),
    );
    wait_for(10 * SECOND, "the input unconnected", || {
        input_status(&foldz)["state"] == "unconnected"
    });
    wait_for(10 * SECOND, "silence at the output", || {
        let k = committed();
        read(k - 50..k).chunks_exact(2 * b).all(is_silent)
    });

    // Scheduled 1 s ahead: silence up to the requested sample rounded up
    // to a block, the fade from there, the pilot once the ramp is over.
    let at = time::now(&api) + 1_000_000_000;
    let requested = format!("{}:{}", at / 1_000_000_000, at % 1_000_000_000);
    stage(
        &foldz,
        "receivers",
        &receiver,
        connect(json!({ "mode": "activate_scheduled_absolute", "requested_time": requested })),
    );
    let index = time::timestamp_to_index(&api, at).next_multiple_of(B);
    let first = index / B;
    let last = (index + u64::from(RAMP)).div_ceil(B) + 10;
    wait_for(10 * SECOND, "the scheduled activation's blocks", || {
        committed() > last
    });
    let blocks = read(first - 10..last);
    let (code, active) = get(
        foldz.nmos,
        &format!("/x-nmos/connection/v1.2/single/receivers/{receiver}/active"),
    );
    assert_eq!(code, 200, "{active}");
    assert_eq!(active["activation"]["activation_time"], requested.as_str());
    assert_eq!(active["transport_params"][0]["mxl_flow_id"], src.as_str());
    for (i, block) in blocks.chunks_exact(2 * b).enumerate() {
        let k = first - 10 + i as u64;
        for ch in 0..2 {
            for n in 0..b {
                let idx = k * B + n as u64;
                let (got, want) = (block[ch * b + n], pilot(ch, idx));
                if idx < index {
                    assert_eq!(got.to_bits(), 0, "channel {ch} at {idx}, switch at {index}");
                } else if idx < index + u64::from(RAMP) {
                    assert!(
                        got.abs() <= want.abs(),
                        "channel {ch} at {idx}: {got} {want}"
                    );
                    if idx > index {
                        assert_eq!(got == 0.0, want == 0.0, "channel {ch} at {idx}: {got}");
                    }
                } else {
                    assert_eq!(got, want, "channel {ch} at {idx}, switch at {index}");
                }
            }
        }
    }

    // A disabled sender's flow is gone; enabled, it is back.
    stage(
        &foldz,
        "senders",
        &sender,
        json!({ "master_enable": false, "activation": immediate() }),
    );
    wait_for(10 * SECOND, "the output flow released", || {
        !domain.has_flow(&out)
    });
    stage(
        &foldz,
        "senders",
        &sender,
        json!({ "master_enable": true, "activation": immediate() }),
    );
    wait_for(10 * SECOND, "the output flow recreated", || {
        domain.has_flow(&out)
    });
    wait_for(10 * SECOND, "the sender's activation applied", || {
        let (_, status) = get(foldz.api, "/api/v1/status");
        let ports = status["nmos"]["ports"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        ports.iter().any(|p| {
            p["kind"] == "sender" && p["applied"] == "applied" && p["masterEnable"] == true
        })
    });
    let (_, status) = get(foldz.api, "/api/v1/status");
    for p in status["nmos"]["ports"].as_array().unwrap() {
        assert_eq!(p["failures"], 0, "{p}");
    }

    // The same seed gives the same ids after a restart.
    assert!(foldz.terminate(), "foldz exits 0 on SIGTERM");
    assert!(!domain.has_flow(&out));
    let foldz = Foldz::start(&file);
    assert_eq!(get(foldz.nmos, "/x-nmos/node/v1.3/self").1["id"], node);
    assert_eq!(resource_id(&foldz, "receivers", "main"), receiver);
    assert_eq!(resource_id(&foldz, "senders", "out"), sender);
    assert!(foldz.terminate(), "foldz exits 0 on SIGTERM");
}
