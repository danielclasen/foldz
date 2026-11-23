//! Passes the engine's heartbeat and counters to the ops listener: the RT
//! thread's heartbeat drives `/healthz`, and the engine, per-input and
//! NMOS counters are rendered on `/metrics`.

use std::time::Duration;

use samplez::Engine;
use samplez_nmos::{Nmos, PortKey};
use samplez_service::{Counter, Metrics, Ops};

use crate::api::{Counters, EngineSlot, InputCounters, connected};

/// How often [`Exporter::run`] samples the engine.
pub const PERIOD: Duration = Duration::from_millis(100);

/// Reads one counter's value since start.
type Value = fn(&Counters) -> u64;

/// The engine counters exported, by metric name: the value since start
/// and the help text. The two maxima never decrease, so they are counters
/// too: samplez-service has no gauge.
const ENGINE: [(&str, Value, &str); 15] = [
    (
        "samplez_rt_heartbeat_blocks_total",
        |c| c.heartbeat_blocks,
        "Blocks the RT thread processed.",
    ),
    (
        "samplez_deadline_miss_total",
        |c| c.deadline_miss,
        "Blocks committed later than their deadline plus a quarter block period.",
    ),
    (
        "samplez_overrun_skipped_samples_total",
        |c| c.overrun_skipped_samples,
        "Samples skipped while more than maxCatchupBlocks behind; libmxl zero-fills them.",
    ),
    (
        "samplez_wait_early_total",
        |c| c.wait_early,
        "Blocks processed before their deadline.",
    ),
    (
        "samplez_input_wait_timeout_total",
        |c| c.input_wait_timeout,
        "Input waits that reached the block deadline.",
    ),
    (
        "samplez_input_late_blocks_total",
        |c| c.input_late_blocks,
        "Input blocks not read because the producer's head was below them.",
    ),
    (
        "samplez_input_silenced_blocks_total",
        |c| c.input_silenced_blocks,
        "Input blocks silenced for any reason.",
    ),
    (
        "samplez_input_probes_total",
        |c| c.input_probes,
        "Validity probes of failed inputs.",
    ),
    (
        "samplez_clock_steps_total",
        |c| c.clock_steps,
        "Steps of libmxl's clock against CLOCK_MONOTONIC.",
    ),
    (
        "samplez_output_held_blocks_total",
        |c| c.output_held_blocks,
        "Output blocks below the last commit, not written.",
    ),
    (
        "samplez_output_failed_writes_total",
        |c| c.output_failed_writes,
        "Output open or commit calls libmxl refused.",
    ),
    (
        "samplez_telemetry_dropped_total",
        |c| c.telemetry_dropped,
        "RT telemetry records dropped on a full ring.",
    ),
    (
        "samplez_rt_minor_faults_total",
        |c| c.rt_minor_faults,
        "Minor page faults of the RT thread.",
    ),
    (
        "samplez_wake_jitter_max_nanoseconds",
        |c| c.wake_jitter_max_ns,
        "Largest wake lateness of the RT thread after its sleep until a block deadline, since start.",
    ),
    (
        "samplez_process_time_max_nanoseconds",
        |c| c.process_time_max_ns,
        "Largest processing time of a block, since start.",
    ),
];

/// Reads one per-input counter's value.
type InputValue = fn(&InputCounters) -> u64;

/// The read outcomes of an input exported as `samplez_input_reads_total`,
/// by `outcome` label.
const OUTCOMES: [(&str, InputValue); 7] = [
    ("copied", |c| c.copied),
    ("below_last_read", |c| c.below_last_read),
    ("late", |c| c.late),
    ("too_late", |c| c.too_late),
    ("too_early", |c| c.too_early),
    ("invalid", |c| c.invalid),
    ("other", |c| c.other),
];

/// One exported counter and the engine value it last added up to.
struct Exported {
    counter: Counter,
    value: Value,
    last: u64,
}

/// One input's exported counters and the values they last added up to.
struct Input {
    name: String,
    series: Vec<(Counter, InputValue, u64)>,
}

impl Input {
    fn new(metrics: &Metrics, name: &str) -> Input {
        let reads = OUTCOMES.iter().map(|(outcome, value)| {
            let labels = [("input", name), ("outcome", *outcome)];
            let help = "Block reads of an input, by outcome.";
            (
                metrics.counter("samplez_input_reads_total", help, &labels),
                *value,
            )
        });
        let probes = (
            metrics.counter(
                "samplez_input_validity_probes_total",
                "Validity probes of an input after a failed read.",
                &[("input", name)],
            ),
            (|c| c.probes) as InputValue,
        );
        Input {
            name: name.to_owned(),
            series: reads
                .chain([probes])
                .map(|(counter, value)| (counter, value, 0))
                .collect(),
        }
    }
}

pub struct Exporter {
    ops: Ops,
    engine: EngineSlot,
    exported: Vec<Exported>,
    inputs: Vec<Input>,
    heartbeat: u64,
    nmos: Option<(Nmos, Vec<(Counter, u64)>)>,
}

/// Adds to `counter` what the source count `now` gained since `last`, and
/// keeps `now` as the next `last`. A count that went back, as a restarted
/// source's does, adds nothing.
fn gain(counter: &mut Counter, last: &mut u64, now: u64) {
    counter.add(now.saturating_sub(*last));
    *last = now;
}

impl Exporter {
    /// With `nmos`, also exports `samplez_nmos_apply_failures_total{port}`
    /// and samplez-nmos's own metrics.
    pub fn new(ops: Ops, engine: EngineSlot, nmos: Option<Nmos>) -> Exporter {
        let metrics: &Metrics = ops.metrics();
        let nmos = nmos.map(|n| {
            n.register_metrics(metrics);
            let failures = n
                .status()
                .ports
                .iter()
                .map(|p| {
                    let port = match &p.key {
                        PortKey::Sender(k) => format!("sender/{k}"),
                        PortKey::Receiver(k) => format!("receiver/{k}"),
                    };
                    let counter = metrics.counter(
                        "samplez_nmos_apply_failures_total",
                        "IS-05 activations the engine did not take on; each turned the port's master_enable false.",
                        &[("port", &port)],
                    );
                    (counter, 0)
                })
                .collect();
            (n, failures)
        });
        let exported = ENGINE
            .iter()
            .map(|(name, value, help)| Exported {
                counter: metrics.counter(name, help, &[]),
                value: *value,
                last: 0,
            })
            .collect();
        Exporter {
            ops,
            engine,
            exported,
            inputs: Vec::new(),
            heartbeat: 0,
            nmos,
        }
    }

    /// Beats the ops heartbeat when the RT thread's has advanced since the
    /// last call, and adds what each engine, input and NMOS counter gained
    /// to its metric. An input's series appear once the engine reads it;
    /// an input never connected has none.
    pub fn sample(&mut self) {
        if let Some((nmos, failures)) = &mut self.nmos {
            for (p, (counter, last)) in nmos.status().ports.iter().zip(failures) {
                gain(counter, last, p.failures);
            }
        }
        let engine = self.engine.lock().expect("engine lock poisoned");
        let Some(engine): Option<&Engine> = engine.as_ref() else {
            return;
        };
        let c = Counters::of(engine.counters());
        if c.heartbeat_blocks != self.heartbeat {
            self.heartbeat = c.heartbeat_blocks;
            self.ops.heartbeat().beat();
        }
        for e in &mut self.exported {
            gain(&mut e.counter, &mut e.last, (e.value)(&c));
        }
        for status in engine.status().inputs {
            let i = match self.inputs.iter().position(|i| i.name == status.name) {
                Some(i) => i,
                None if connected(&status) => {
                    self.inputs
                        .push(Input::new(self.ops.metrics(), &status.name));
                    self.inputs.len() - 1
                }
                None => continue,
            };
            let counters = InputCounters::of(&status);
            for (counter, value, last) in &mut self.inputs[i].series {
                gain(counter, last, value(&counters));
            }
        }
    }

    /// Samples every [`PERIOD`]; never returns.
    pub async fn run(mut self) {
        let mut ticks = tokio::time::interval(PERIOD);
        loop {
            ticks.tick().await;
            self.sample();
        }
    }
}
