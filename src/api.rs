//! The read-only `/api/v1` routes: the downmix as configured and the
//! engine's status, and their OpenAPI document. foldz takes no commands:
//! its shape, layouts and matrices are fixed at start-up.

use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use samplez::rt::Step;
use samplez::{Engine, InputState};
use samplez_nmos::{Applied, Nmos, PortKey};
use samplez_service::StatusSource;
use samplez_service::config::TlsMode;
use serde::Serialize;
use utoipa::{OpenApi, ToSchema};

use crate::config::{Config, UNCONNECTED_FLOW};
use crate::presets::{Fold, Lfe, PresetName};

/// The running engine; `None` once it stopped.
pub type EngineSlot = Arc<Mutex<Option<Engine>>>;

/// The downmix as configured: every input and every output with the gain
/// matrix the engine applies to it.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Downmix {
    pub inputs: Vec<DownmixInput>,
    pub outputs: Vec<DownmixOutput>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DownmixInput {
    pub name: String,
    pub domain: String,
    /// The MXL flow read at start-up; `null` starts the input
    /// unconnected.
    pub flow: Option<String>,
    /// IS-04 channel symbols, `-` for an unused channel.
    pub layout: Vec<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DownmixOutput {
    pub name: String,
    pub input: String,
    pub domain: String,
    /// The MXL flow written.
    pub flow: String,
    pub label: String,
    /// The flow's grouphint tag, `<group>:<role>`.
    pub grouphint: String,
    pub layout: Vec<String>,
    /// `null` for an explicit matrix.
    pub preset: Option<String>,
    /// The document and table a published preset's coefficients come from.
    pub citation: Option<String>,
    /// The input's LFE added to every output channel at this gain in dB
    /// before `gainDb`; `null` when it is not added.
    pub lfe_gain_db: Option<f64>,
    pub gain_db: f64,
    /// The gains applied, `gainDb` included: one row per output channel,
    /// one column per input channel.
    pub matrix: Vec<Vec<f32>>,
}

impl Downmix {
    /// From a config and the folds [`Config::validate`] returned for it.
    pub fn new(config: &Config, folds: &[Fold]) -> Downmix {
        let only_domain = || config.domains[0].name.clone();
        let labels = |l: &crate::layout::Layout| l.labels().iter().map(|l| l.to_string()).collect();
        let inputs = config
            .inputs
            .iter()
            .map(|i| DownmixInput {
                name: i.name.clone(),
                domain: i.domain.clone().unwrap_or_else(only_domain),
                flow: i.flow.clone(),
                layout: labels(&i.layout),
            })
            .collect();
        let outputs = config
            .outputs
            .iter()
            .zip(folds)
            .map(|(o, f)| {
                let preset = o.preset.map(|p| p.to_string());
                let citation = match o.preset {
                    Some(PresetName::Table(p)) => Some(p.citation().to_owned()),
                    _ => None,
                };
                let lfe_gain_db = match o.lfe {
                    Some(Lfe::Include(include)) => Some(include.gain_db),
                    _ => None,
                };
                let m = &f.matrix;
                DownmixOutput {
                    name: o.name.clone(),
                    input: o
                        .input
                        .clone()
                        .unwrap_or_else(|| config.inputs[0].name.clone()),
                    domain: o.domain.clone().unwrap_or_else(only_domain),
                    flow: o.flows[0].clone(),
                    label: o.label().to_owned(),
                    grouphint: format!("{}:{}", o.group(), o.role()),
                    layout: labels(&o.layout),
                    preset,
                    citation,
                    lfe_gain_db,
                    gain_db: o.gain_db,
                    matrix: (0..m.outputs())
                        .map(|j| (0..m.inputs()).map(|i| m.gain(j, i)).collect())
                        .collect(),
                }
            })
            .collect();
        Downmix { inputs, outputs }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// The RT setup steps, in the order they run: capabilities, dumpable,
    /// mlock, stack, sched, affinity, ftzDaz, signals. A degraded step
    /// leaves the engine running without it.
    pub rt: Vec<RtStep>,
    /// Why the output no longer follows from the inputs.
    pub diverged: Option<String>,
    /// The message of a panic on the RT thread; the engine stopped
    /// processing and deleted its output flows.
    pub rt_panic: Option<String>,
    pub inputs: Vec<InputStatus>,
    pub outputs: Vec<OutputStatus>,
    pub counters: Counters,
    /// One per listener that runs TLS.
    pub tls: Vec<TlsStatus>,
    /// `null` with NMOS disabled.
    pub nmos: Option<NmosStatus>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NmosStatus {
    /// The Registration API the node is registered with.
    pub registry: Option<String>,
    /// Every sender, then every receiver.
    pub ports: Vec<NmosPort>,
}

/// One sender (an output) or receiver (an input) and its IS-05 `/active`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NmosPort {
    pub kind: PortKind,
    /// The output's or input's name.
    pub name: String,
    pub master_enable: bool,
    pub mxl_domain_id: Option<String>,
    pub mxl_flow_id: Option<String>,
    /// TAI `<seconds>:<nanoseconds>` of a scheduled activation not fired
    /// yet.
    pub pending: Option<String>,
    pub applied: AppliedState,
    /// Why the last activation failed; it turned `masterEnable` false.
    pub error: Option<String>,
    /// Activations that failed since start.
    pub failures: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum PortKind {
    Sender,
    Receiver,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum AppliedState {
    /// Nothing activated since start.
    Initial,
    Applying,
    Applied,
    Failed,
}

impl NmosStatus {
    pub fn of(nmos: &Nmos) -> NmosStatus {
        let s = nmos.status();
        let ports = s
            .ports
            .into_iter()
            .map(|p| {
                let (kind, name) = match p.key {
                    PortKey::Sender(k) => (PortKind::Sender, k),
                    PortKey::Receiver(k) => (PortKind::Receiver, k),
                };
                let (applied, error) = match p.applied {
                    Applied::Initial => (AppliedState::Initial, None),
                    Applied::Applying => (AppliedState::Applying, None),
                    Applied::Applied => (AppliedState::Applied, None),
                    Applied::Failed(why) => (AppliedState::Failed, Some(why)),
                };
                NmosPort {
                    kind,
                    name,
                    master_enable: p.active.master_enable,
                    mxl_domain_id: p.active.domain.map(|d| d.to_string()),
                    mxl_flow_id: p.active.flow.map(|f| f.to_string()),
                    pending: p.pending.map(|t| format!("{}:{}", t.sec, t.nsec)),
                    applied,
                    error,
                    failures: p.failures,
                }
            })
            .collect();
        NmosStatus {
            registry: s.registry,
            ports,
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RtStep {
    pub name: String,
    pub state: StepState,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum StepState {
    /// Done as configured.
    Active,
    /// Not done or done partly.
    Degraded,
    /// Not configured or not applicable.
    Off,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InputStatus {
    pub name: String,
    /// `unconnected` for an input without a flow, which is not read.
    pub state: InputStateName,
    /// The MXL flow read; `null` when unconnected.
    pub flow: Option<String>,
    /// Why the last attach failed.
    pub error: Option<String>,
    /// The margin in samples the producer's declared commit batch needs,
    /// when `readerMarginSamples` is below it.
    pub margin_short: Option<u64>,
    /// The margin in samples the observed late blocks need.
    pub margin_warning: Option<u64>,
    /// Per-block read outcomes.
    pub counters: InputCounters,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum InputStateName {
    /// Read by the RT thread.
    Attached,
    /// Not read; polled until its producer is on time again.
    Pruned,
    Unconnected,
}

#[derive(Debug, Clone, Default, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InputCounters {
    pub copied: u64,
    pub below_last_read: u64,
    pub late: u64,
    pub too_late: u64,
    pub too_early: u64,
    pub invalid: u64,
    pub other: u64,
    pub probes: u64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OutputStatus {
    pub name: String,
    /// Whether the output's flow is created and written.
    pub realised: bool,
    /// Why it is not.
    pub reason: Option<String>,
    pub attempts: u64,
    /// Creations that found the flow created by another writer first.
    pub races: u64,
    pub create_failures: u64,
    /// The flow's ring length in samples, once created.
    pub buffer_length: Option<u64>,
    /// The effective `maxCommitBatchSizeHint` and `maxSyncBatchSizeHint`.
    pub max_commit_batch_size_hint: Option<u32>,
    pub max_sync_batch_size_hint: Option<u32>,
}

/// The engine counters since start.
#[derive(Debug, Clone, Default, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Counters {
    pub heartbeat_blocks: u64,
    pub deadline_miss: u64,
    pub overrun_skipped_samples: u64,
    pub wait_early: u64,
    pub input_wait_timeout: u64,
    pub input_late_blocks: u64,
    pub input_silenced_blocks: u64,
    pub input_probes: u64,
    pub clock_steps: u64,
    pub output_held_blocks: u64,
    pub output_failed_writes: u64,
    pub telemetry_dropped: u64,
    pub wake_jitter_max_ns: u64,
    pub process_time_max_ns: u64,
    pub rt_minor_faults: u64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TlsStatus {
    pub listener: String,
    /// `files` or `selfSigned`.
    pub mode: String,
    /// SHA-256 of the certificate in use, colon-separated hex pairs.
    pub fingerprint: String,
    /// Seconds since the Unix epoch of the last read or generation.
    pub loaded_at: u64,
    /// Seconds since the Unix epoch at which the certificate expires.
    pub not_after: Option<u64>,
}

impl Counters {
    pub fn of(c: &samplez::Counters) -> Counters {
        use std::sync::atomic::Ordering::Relaxed;
        Counters {
            heartbeat_blocks: c.heartbeat_blocks.load(Relaxed),
            deadline_miss: c.deadline_miss.load(Relaxed),
            overrun_skipped_samples: c.overrun_skipped_samples.load(Relaxed),
            wait_early: c.wait_early.load(Relaxed),
            input_wait_timeout: c.input_wait_timeout.load(Relaxed),
            input_late_blocks: c.input_late_blocks.load(Relaxed),
            input_silenced_blocks: c.input_silenced_blocks.load(Relaxed),
            input_probes: c.input_probes.load(Relaxed),
            clock_steps: c.clock_steps.load(Relaxed),
            output_held_blocks: c.output_held_blocks.load(Relaxed),
            output_failed_writes: c.output_failed_writes.load(Relaxed),
            telemetry_dropped: c.telemetry_dropped.load(Relaxed),
            wake_jitter_max_ns: c.wake_jitter_max_ns.load(Relaxed),
            process_time_max_ns: c.process_time_max_ns.load(Relaxed),
            rt_minor_faults: c.rt_minor_faults.load(Relaxed),
        }
    }
}

impl InputCounters {
    pub fn of(e: &samplez::InputStatus) -> InputCounters {
        let c = &e.counters;
        InputCounters {
            copied: c.copied,
            below_last_read: c.below_last_read,
            late: c.late,
            too_late: c.too_late,
            too_early: c.too_early,
            invalid: c.invalid,
            other: c.other,
            probes: c.probes,
        }
    }
}

fn rt_step(name: &str, step: &Step) -> RtStep {
    let (state, detail) = match step {
        Step::Active(d) => (StepState::Active, d),
        Step::Degraded(d) => (StepState::Degraded, d),
        Step::Off(d) => (StepState::Off, d),
    };
    RtStep {
        name: name.to_owned(),
        state,
        detail: detail.clone(),
    }
}

/// Whether the engine reads input `e`: an input left on
/// [`UNCONNECTED_FLOW`] until its detach applies reads nothing either.
pub fn connected(e: &samplez::InputStatus) -> bool {
    e.state != InputState::Detached && e.flow_id != UNCONNECTED_FLOW
}

impl Status {
    /// The engine's status; `config` names the inputs, in config order.
    pub fn of(engine: &Engine, config: &Config, tls: &StatusSource, nmos: Option<&Nmos>) -> Status {
        let s = engine.status();
        let (p, t) = (&s.rt.process, &s.rt.thread);
        let rt = [
            ("capabilities", &p.capabilities),
            ("dumpable", &p.dumpable),
            ("mlock", &p.mlock),
            ("stack", &p.stack),
            ("sched", &t.sched),
            ("affinity", &t.affinity),
            ("ftzDaz", &t.ftz_daz),
            ("signals", &t.signals),
        ]
        .into_iter()
        .map(|(name, step)| rt_step(name, step))
        .collect();
        let inputs = config
            .inputs
            .iter()
            .map(|i| match s.inputs.iter().find(|e| e.name == i.name) {
                Some(e) if connected(e) => InputStatus {
                    name: i.name.clone(),
                    state: if e.state == InputState::Pruned {
                        InputStateName::Pruned
                    } else {
                        InputStateName::Attached
                    },
                    flow: Some(e.flow_id.clone()),
                    error: e.error.clone(),
                    margin_short: e.margin_short,
                    margin_warning: e.margin_warning,
                    counters: InputCounters::of(e),
                },
                _ => InputStatus {
                    name: i.name.clone(),
                    state: InputStateName::Unconnected,
                    flow: None,
                    error: None,
                    margin_short: None,
                    margin_warning: None,
                    counters: InputCounters::default(),
                },
            })
            .collect();
        let outputs = s
            .outputs
            .iter()
            .map(|o| OutputStatus {
                name: o.name.clone(),
                realised: o.realised,
                reason: o.reason.clone(),
                attempts: o.attempts,
                races: o.races,
                create_failures: o.create_failures,
                buffer_length: o.buffer_length,
                max_commit_batch_size_hint: o.hints.map(|h| h.0),
                max_sync_batch_size_hint: o.hints.map(|h| h.1),
            })
            .collect();
        let tls = tls
            .get()
            .tls
            .into_iter()
            .map(|t| TlsStatus {
                listener: t.listener.to_owned(),
                mode: match t.mode {
                    TlsMode::None => "none",
                    TlsMode::Files => "files",
                    TlsMode::SelfSigned => "selfSigned",
                }
                .to_owned(),
                fingerprint: t.fingerprint,
                loaded_at: t.loaded_at,
                not_after: t.not_after,
            })
            .collect();
        Status {
            rt,
            diverged: s.diverged,
            rt_panic: s.rt_panic,
            inputs,
            outputs,
            counters: Counters::of(engine.counters()),
            tls,
            nmos: nmos.map(NmosStatus::of),
        }
    }
}

struct App {
    engine: EngineSlot,
    config: Config,
    downmix: Downmix,
    tls: StatusSource,
    nmos: Option<Nmos>,
}

/// The downmix as configured.
#[utoipa::path(
    get,
    path = "/downmix",
    tag = "foldz",
    responses((status = 200, description = "The downmix", body = Downmix))
)]
async fn downmix(State(app): State<Arc<App>>) -> Json<Downmix> {
    Json(app.downmix.clone())
}

/// The engine's RT setup, inputs, outputs and counters, and the NMOS
/// connections.
#[utoipa::path(
    get,
    path = "/status",
    tag = "foldz",
    responses(
        (status = 200, description = "The engine's status", body = Status),
        (status = 503, description = "The engine has stopped")
    )
)]
async fn status(State(app): State<Arc<App>>) -> Response {
    let engine = app.engine.lock().expect("engine lock poisoned");
    match engine.as_ref() {
        Some(e) => Json(Status::of(e, &app.config, &app.tls, app.nmos.as_ref())).into_response(),
        None => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

/// The routes under `/api/v1`.
pub fn router(
    engine: EngineSlot,
    config: Config,
    downmix: Downmix,
    tls: StatusSource,
    nmos: Option<Nmos>,
) -> Router {
    let app = Arc::new(App {
        engine,
        config,
        downmix,
        tls,
        nmos,
    });
    Router::new()
        .route("/downmix", get(self::downmix))
        .route("/status", get(self::status))
        .with_state(app)
}

#[derive(OpenApi)]
#[openapi(
    info(title = "foldz", version = "1", description = "Read-only foldz API"),
    paths(downmix, status)
)]
struct Api;

/// The OpenAPI document of `/api/v1` in auth mode `mode`.
pub fn openapi(mode: samplez_service::config::AuthMode) -> utoipa::openapi::OpenApi {
    samplez_service::openapi::document(Api::openapi(), mode)
}
