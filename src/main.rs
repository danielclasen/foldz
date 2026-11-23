//! `foldz --config <file>`: runs the downmix until SIGINT or SIGTERM, then
//! deletes its NMOS node from the registry and releases its writers, which
//! deletes their flows.

use std::net::IpAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use foldz::api::{self, Downmix};
use foldz::config::Config;
use foldz::metrics::Exporter;
use foldz::state::Standalone;
use samplez::Engine;
use samplez_service::{Ops, Service, ServiceConfig};

/// The stack of the thread that serves the listeners. Threads get
/// samplez's capped default otherwise.
const SERVICE_STACK: usize = 2 * 1024 * 1024;

/// How long SIGTERM waits for the registry to answer the Node's DELETE,
/// well inside a pod's default 30 s termination grace period, so the
/// writers are released and their flows deleted before a SIGKILL.
const UNREGISTER_WAIT: Duration = Duration::from_secs(5);

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
    // SAFETY: sigemptyset initialises the set before anything reads it,
    // and the set lives through every call that takes it.
    let set = unsafe {
        let mut set = std::mem::MaybeUninit::<libc::sigset_t>::uninit();
        libc::sigemptyset(set.as_mut_ptr());
        let mut set = set.assume_init();
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
    // samplez-service reports its errors as config errors: the auth and
    // TLS settings, the files they name, and the random bits drawn for
    // certificates and session ids.
    let service = match Service::new(service_config, ops.clone(), state) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("foldz: config: {e}");
            return ExitCode::from(2);
        }
    };
    let slot: api::EngineSlot = Arc::new(Mutex::new(None));
    let nmos = if config.nmos.enabled {
        match foldz::nmos::node(&config, &folds, slot.clone()) {
            Ok(n) => Some(n),
            Err(e) => {
                eprintln!("foldz: nmos: {e}");
                return ExitCode::from(2);
            }
        }
    } else {
        None
    };
    if let Some(n) = &nmos {
        eprintln!("foldz: nmos node {}", n.node_id());
        let bind = config.listeners.nmos.bind;
        let host = &config.nmos.advertise_host;
        let loopback = host == "localhost" || host.parse::<IpAddr>().is_ok_and(|a| a.is_loopback());
        if bind.is_loopback() && !loopback {
            eprintln!(
                "foldz: warning: the NMOS listener is bound to {bind}, which {host} does not reach"
            );
        }
    }
    let downmix = Downmix::new(&config, &folds);
    let tls = service.status();
    let router = api::router(
        slot.clone(),
        config.clone(),
        downmix,
        tls.clone(),
        nmos.clone(),
    );
    let mut service = service.api(router);
    if let Some(n) = &nmos {
        service = service.nmos(n.router());
    }

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
    // An input without a flow was started on UNCONNECTED_FLOW and reads
    // nothing from here on.
    for (i, input) in config.inputs.iter().enumerate() {
        if input.flow.is_none()
            && let Err(e) = engine.detach(i, 0)
        {
            eprintln!("foldz: engine: detaching input {}: {e}", input.name);
            engine.stop();
            return ExitCode::FAILURE;
        }
    }
    let status = api::Status::of(&engine, &config, &tls, nmos.as_ref());
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

    let exporter = Exporter::new(ops.clone(), slot.clone(), nmos.clone());
    // Set before the service thread signals, so it is set when sigwait
    // returns for that signal.
    let serve_failed = Arc::new(AtomicBool::new(false));
    let failed = serve_failed.clone();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let served = std::thread::Builder::new()
        .name("foldz-service".into())
        .stack_size(SERVICE_STACK)
        .spawn(move || {
            // A panic below ends the sigwait in main as a normal end does,
            // so main joins this thread, sees the panic and exits at once.
            // SAFETY: SIGTERM to this process ends the sigwait below.
            let _wake = OnPanic(|| unsafe {
                libc::kill(libc::getpid(), libc::SIGTERM);
            });
            let (result, stopping) = runtime.block_on(async {
                let registration = async {
                    if let Some(n) = &nmos {
                        n.registration().await;
                    }
                    std::future::pending::<()>().await
                };
                let (result, stopping) = tokio::select! {
                    r = bound.serve() => (r, false),
                    () = exporter.run() => unreachable!("the exporter never returns"),
                    () = registration => unreachable!("registration never returns"),
                    _ = stopped => (Ok(()), true),
                };
                // Registration is dropped, so no POST lands after the
                // DELETE.
                if let Some(n) = &nmos
                    && tokio::time::timeout(UNREGISTER_WAIT, n.unregister())
                        .await
                        .is_err()
                {
                    eprintln!("foldz: nmos: the registry did not answer the node's DELETE");
                }
                (result, stopping)
            });
            if let Err(e) = result {
                eprintln!("foldz: serving: {e}");
                failed.store(true, Ordering::SeqCst);
            }
            if !stopping {
                // SAFETY: SIGTERM to this process ends the sigwait below.
                unsafe { libc::kill(libc::getpid(), libc::SIGTERM) };
            }
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
    // The node leaves the registry before its flows go.
    let _ = stop.send(());
    // A service thread that panicked ends the process as a failure, as
    // one whose serving failed does.
    let panicked = served.join().is_err();
    if panicked {
        eprintln!("foldz: service thread panicked");
    }
    ops.set_engine_started(false);
    // The guard is dropped before stop: the listeners lock the slot too,
    // and would wait for stop to return.
    let engine = slot.lock().expect("engine lock poisoned").take();
    if let Some(engine) = engine {
        engine.stop();
    }
    if panicked || serve_failed.load(Ordering::SeqCst) {
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

/// Runs its action when dropped while its thread panics, and not
/// otherwise.
struct OnPanic<F: FnMut()>(F);

impl<F: FnMut()> Drop for OnPanic<F> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            (self.0)();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OnPanic;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// The guard acts on a panic of its thread and on nothing else, so a
    /// service thread that ends normally does not signal the process.
    #[test]
    fn the_guard_acts_only_on_a_panic() {
        for panics in [false, true] {
            let fired = Arc::new(AtomicBool::new(false));
            let seen = fired.clone();
            let joined = std::thread::spawn(move || {
                let _guard = OnPanic(|| seen.store(true, Ordering::SeqCst));
                assert!(!panics, "the service thread panicked");
            })
            .join();
            assert_eq!(joined.is_err(), panics);
            assert_eq!(fired.load(Ordering::SeqCst), panics, "panics {panics}");
        }
    }
}
