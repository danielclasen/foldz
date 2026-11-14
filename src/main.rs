//! `foldz --config <file>`: runs the downmix until SIGINT or SIGTERM, then
//! releases its writers, which deletes their flows.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use foldz::api::{self, Downmix};
use foldz::config::Config;
use foldz::metrics::Exporter;
use foldz::state::Standalone;
use samplez::Engine;
use samplez_service::{Ops, Service, ServiceConfig};

/// The stack of the thread that serves the listeners. Threads get
/// samplez's capped default otherwise.
const SERVICE_STACK: usize = 2 * 1024 * 1024;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let path = match args.as_slice() {
        [_, flag, path] if flag == "--config" => PathBuf::from(path),
        [_, flag] if flag == "--version" => {
            println!("foldz {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        _ => {
            eprintln!("usage: foldz --config <file>");
            return ExitCode::from(2);
        }
    };
    let (config, folds) = match Config::load(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("foldz: config: {e}");
            return ExitCode::from(2);
        }
    };
    let engine_config = match config.engine(&folds) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("foldz: config: {e}");
            return ExitCode::from(2);
        }
    };
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    // SIGINT and SIGTERM are blocked before any thread exists, so every
    // thread inherits the mask and only sigwait below receives them.
    // SAFETY: `set` is initialised by sigemptyset before use.
    let set = unsafe {
        let mut set = std::mem::zeroed::<libc::sigset_t>();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGINT);
        libc::sigaddset(&mut set, libc::SIGTERM);
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
        set
    };
    let process = samplez::rt::setup_process(&config.rt);

    let ops = Ops::default();
    let service_config = ServiceConfig {
        listeners: config.listeners.clone(),
        auth: config.auth.clone(),
        api: config.api.clone(),
    };
    let state = Arc::new(Standalone::new(hostname()));
    let service = match Service::new(service_config, ops.clone(), state) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("foldz: service: {e}");
            return ExitCode::FAILURE;
        }
    };
    let slot: api::EngineSlot = Arc::new(Mutex::new(None));
    let downmix = Downmix::new(&config, &folds);
    let tls = service.status();
    let router = api::router(slot.clone(), config.clone(), downmix, tls.clone());
    let service = service.api(router);

    // A current-thread runtime starts no thread, so the listeners are bound
    // here and a taken port stops the start before any flow is created.
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("foldz: runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    let bound = match runtime.block_on(service.bind()) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("foldz: binding the listeners: {e}");
            return ExitCode::FAILURE;
        }
    };
    for (name, addr) in [
        ("api", Some(bound.api_addr())),
        ("machine", bound.machine_addr()),
        ("ops", Some(bound.ops_addr())),
        ("nmos", Some(bound.nmos_addr())),
    ] {
        if let Some(Ok(addr)) = addr {
            eprintln!("foldz: {name} listener on {addr}");
        }
    }

    let engine = match Engine::start(engine_config, process, None) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("foldz: engine: {e}");
            return ExitCode::FAILURE;
        }
    };
    let status = api::Status::of(&engine, &config, &tls);
    for step in status.rt {
        eprintln!("foldz: rt {}: {:?}: {}", step.name, step.state, step.detail);
    }
    for t in status.tls {
        eprintln!(
            "foldz: {} listener TLS {}: certificate SHA-256 {}",
            t.listener, t.mode, t.fingerprint
        );
    }
    *slot.lock().expect("engine lock poisoned") = Some(engine);
    ops.set_engine_started(true);

    let exporter = Exporter::new(ops.clone(), slot.clone());
    let served = std::thread::Builder::new()
        .name("foldz-service".into())
        .stack_size(SERVICE_STACK)
        .spawn(move || {
            let result = runtime.block_on(async {
                tokio::select! {
                    r = bound.serve() => r,
                    () = exporter.run() => unreachable!("the exporter never returns"),
                }
            });
            if let Err(e) = &result {
                eprintln!("foldz: serving: {e}");
            }
            // SAFETY: SIGTERM to this process ends the sigwait below.
            unsafe { libc::kill(libc::getpid(), libc::SIGTERM) };
            result
        });
    let served = match served {
        Ok(t) => t,
        Err(e) => {
            eprintln!("foldz: service thread: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut sig = 0;
    // SAFETY: `set` is a valid signal set and `sig` is writable.
    unsafe { libc::sigwait(&set, &mut sig) };
    eprintln!("foldz: signal {sig}, stopping");
    ops.set_engine_started(false);
    if let Some(engine) = slot.lock().expect("engine lock poisoned").take() {
        engine.stop();
    }
    if served.is_finished() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// The host name, the replica id in `/healthz`, `/readyz` and `/metrics`.
fn hostname() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: `buf` is writable for its length.
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    match std::str::from_utf8(&buf[..end]) {
        Ok(name) if rc == 0 && !name.is_empty() => name.to_owned(),
        _ => "foldz".to_owned(),
    }
}
