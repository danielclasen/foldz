//! foldz as an NMOS node through samplez-nmos: each input is an MXL
//! receiver and each output an MXL sender (AMWA BCP-007-03). An IS-05
//! activation routes or detaches the input's engine slot, or enables or
//! disables the output's writer, at the activation's sample index.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use samplez::Engine;
use samplez_mxl::{Instance, MxlApi};
use samplez_nmos::uuid::Uuid;
use samplez_nmos::{
    Active, Asset, BoxFuture, ChannelCount, ChannelDesc, Connections, Fail, FlowSeen, Nmos,
    NmosConfig, NoStore, NodeModel, Port, ReceiverDesc, Reject, SenderDesc, SystemTai, Tai,
};
use serde_json::Value;

use crate::api::EngineSlot;
use crate::config::{Config, ConfigError, Domain, is_lowercase_uuid};
use crate::layout::{Label, Layout};

/// How long an enabled output's writer may take to come in before the
/// activation counts as failed. Within samplez-nmos's apply timeout.
pub const CREATE_TIMEOUT: Duration = Duration::from_secs(5);

/// Added to the engine's buildLead for [`Connections::lead`]: the timer
/// that calls `apply` may wake late.
const LEAD_SLACK: Duration = Duration::from_millis(10);

fn err<T>(at: impl Into<String>, reason: impl Into<String>) -> Result<T, ConfigError> {
    Err(ConfigError {
        at: at.into(),
        reason: reason.into(),
    })
}

/// The MXL id of each configured domain, in config order: the `id` of the
/// domain's `domain_def.json` when the file is there, else `domains[].id`.
/// Both present and different, or neither present, is an error.
pub fn domain_ids(domains: &[Domain]) -> Result<Vec<Uuid>, ConfigError> {
    domains
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let at = format!("domains[{i}]");
            let file = Path::new(&d.path).join("domain_def.json");
            let shown = file.display().to_string();
            let from_file = match std::fs::read_to_string(&file) {
                Ok(text) => match def_id(&text) {
                    Some(id) => Some(id),
                    None => return err(&shown, "has no lowercase UUID \"id\""),
                },
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return err(&shown, e.to_string()),
            };
            let id = match (from_file, d.id.clone()) {
                (Some(file_id), Some(id)) if file_id != id => {
                    return err(
                        format!("{at}.id"),
                        format!("{id} differs from the id {file_id} in {shown}"),
                    );
                }
                (Some(id), _) | (None, Some(id)) => id,
                (None, None) => {
                    return err(
                        format!("{at}.id"),
                        format!("required with NMOS enabled: {shown} does not exist"),
                    );
                }
            };
            id.parse()
                .or_else(|e| err(format!("{at}.id"), format!("{id}: {e}")))
        })
        .collect()
}

/// The `id` of a `domain_def.json`.
fn def_id(text: &str) -> Option<String> {
    let v: Value = serde_json::from_str(text).ok()?;
    let id = v.get("id")?.as_str()?;
    is_lowercase_uuid(id).then(|| id.to_owned())
}

/// The index of each item's domain among `config.domains`.
fn domain_index(config: &Config, given: Option<&str>) -> usize {
    match given {
        Some(name) => config
            .domains
            .iter()
            .position(|d| d.name == name)
            .expect("validate resolved the domain"),
        None => 0,
    }
}

fn channels(layout: &Layout) -> Vec<ChannelDesc> {
    layout
        .labels()
        .iter()
        .map(|l| match l {
            Label::Unused => ChannelDesc {
                label: "unused".to_owned(),
                symbol: None,
            },
            l => ChannelDesc {
                label: l.to_string(),
                symbol: Some(l.to_string()),
            },
        })
        .collect()
}

fn flow_uuid(id: &str) -> Uuid {
    id.parse().expect("validate checked the flow id")
}

/// One Sender per output and one Receiver per input, keyed and grouped by
/// name; inputs and outputs share no name.
pub fn model(config: &Config, domains: &[Uuid]) -> NodeModel {
    let senders = config
        .outputs
        .iter()
        .map(|o| SenderDesc {
            key: o.name.clone(),
            label: o.label().to_owned(),
            channels: channels(&o.layout),
            group: o.name.clone(),
            domain: domains[domain_index(config, o.domain.as_deref())],
            flow: flow_uuid(&o.flows[0]),
        })
        .collect();
    let receivers = config
        .inputs
        .iter()
        .map(|i| ReceiverDesc {
            key: i.name.clone(),
            label: i.name.clone(),
            channels: ChannelCount::Exactly(i.channels as u32),
            group: i.name.clone(),
            domain: domains[domain_index(config, i.domain.as_deref())],
            flow: i.flow.as_deref().map(flow_uuid),
        })
        .collect();
    NodeModel {
        label: format!("foldz {}", config.nmos.seed()),
        description: "foldz downmix and channel matrix".to_owned(),
        senders,
        receivers,
    }
}

pub fn nmos_config(config: &Config) -> NmosConfig {
    let n = &config.nmos;
    let port = match n.advertise_port {
        0 => config.listeners.nmos.port,
        p => p,
    };
    NmosConfig {
        seed: n.seed().to_owned(),
        advertise_host: n.advertise_host.clone(),
        advertise_port: port,
        asset: Asset {
            manufacturer: n.asset.manufacturer.clone(),
            product: "foldz".to_owned(),
            instance_id: n.seed().to_owned(),
            functions: vec!["Downmix".to_owned()],
        },
        heartbeat: Duration::from_secs(n.heartbeat_s),
        registries: (!n.registry.url.is_empty())
            .then(|| n.registry.url.trim_end_matches('/').to_owned())
            .into_iter()
            .collect(),
        dns_sd_domain: (!n.registry.dns_sd_domain.is_empty())
            .then(|| n.registry.dns_sd_domain.clone()),
    }
}

/// The NMOS node of `config`, whose activations act on the engine in
/// `engine` once it is there. Loads libmxl to read flow definitions and
/// to turn activation times into sample indices.
pub fn node(config: &Config, engine: EngineSlot) -> Result<Nmos, String> {
    let ids = domain_ids(&config.domains).map_err(|e| e.to_string())?;
    let api = samplez_mxl::load(&config.mxl.library_path).map_err(|e| format!("libmxl: {e}"))?;
    let instances = config
        .domains
        .iter()
        .map(|d| {
            Instance::new(&api, Path::new(&d.path))
                .map_err(|e| format!("MXL domain {}: {e}", d.path))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let inputs = config
        .inputs
        .iter()
        .map(|i| instances[domain_index(config, i.domain.as_deref())].clone())
        .collect();
    let ports = EnginePorts {
        engine,
        api,
        block: u64::from(config.engine.block_size),
        inputs,
    };
    Nmos::new(
        model(config, &ids),
        nmos_config(config),
        Arc::new(ports),
        &NoStore,
        SystemTai,
    )
    .map_err(|e| e.to_string())
}

/// What `domain` holds under `flow`, read from its flow definition.
pub fn flow_seen(instance: &Instance, flow: &str) -> Result<FlowSeen, Reject> {
    let reject = |e: &dyn std::fmt::Display| Reject(format!("flow {flow}: {e}"));
    if instance
        .is_flow_active(flow)
        .map_err(|e| reject(&e))?
        .is_none()
    {
        return Ok(FlowSeen::Absent);
    }
    let def = instance.flow_def(flow).map_err(|e| reject(&e))?;
    parse_flow_def(&def).ok_or_else(|| reject(&"not an audio flow definition"))
}

/// The channels, rate and sample format of an audio flow definition.
pub fn parse_flow_def(def: &str) -> Option<FlowSeen> {
    let v: Value = serde_json::from_str(def).ok()?;
    if v.get("format")?.as_str()? != "urn:x-nmos:format:audio" {
        return None;
    }
    let rate = v.get("sample_rate")?;
    let numerator = rate.get("numerator")?.as_u64()?;
    let denominator = match rate.get("denominator") {
        Some(d) => d.as_u64()?,
        None => 1,
    };
    if denominator == 0 {
        return None;
    }
    Some(FlowSeen::Present {
        channels: u32::try_from(v.get("channel_count")?.as_u64()?).ok()?,
        // A rate that is no whole number of samples per second is no rate
        // a receiver takes: rounded down, it fails the caps.
        rate: u32::try_from(numerator / denominator).ok()?,
        float32: v.get("media_type").and_then(Value::as_str) == Some("audio/float32"),
    })
}

/// The IS-05 hooks on a running engine.
struct EnginePorts {
    engine: EngineSlot,
    api: Arc<MxlApi>,
    block: u64,
    /// Each input's domain instance, in input order.
    inputs: Vec<Arc<Instance>>,
}

impl EnginePorts {
    fn with_engine<T>(&self, f: impl FnOnce(&Engine) -> T) -> Result<T, Fail> {
        let engine = self.engine.lock().expect("engine lock poisoned");
        engine
            .as_ref()
            .map(f)
            .ok_or_else(|| Fail("the engine is not running".to_owned()))
    }

    /// The first block-aligned sample index at or after TAI time `at`.
    fn index(&self, at: Tai) -> u64 {
        let ns = u64::try_from(at.as_nanos()).unwrap_or(u64::MAX);
        samplez_mxl::time::timestamp_to_index(&self.api, ns).next_multiple_of(self.block)
    }

    fn apply_receiver(&self, slot: usize, active: &Active, at_index: u64) -> Result<(), Fail> {
        let applied = self.with_engine(|e| match (active.master_enable, active.flow) {
            (true, Some(flow)) => e.route(slot, &flow.to_string(), at_index),
            _ => e.detach(slot, at_index),
        })?;
        applied.map(drop).map_err(|e| {
            // samplez-nmos turns the receiver's master_enable false in
            // /active, so the slot reads nothing either.
            let _ = self.with_engine(|en| en.detach(slot, 0));
            Fail(e.to_string())
        })
    }

    async fn apply_sender(&self, output: usize, enabled: bool, at_index: u64) -> Result<(), Fail> {
        self.with_engine(|e| e.set_output_enabled(output, enabled, at_index))?
            .map_err(|e| Fail(e.to_string()))?;
        if !enabled {
            return Ok(());
        }
        // Applied once the writer is in. A flow another writer holds keeps
        // it out; the engine retries, which within CREATE_TIMEOUT counts.
        let deadline = Instant::now() + CREATE_TIMEOUT;
        loop {
            let (realised, reason) = self.with_engine(|e| {
                let s = &e.status().outputs[output];
                (s.enabled && s.realised, s.reason.clone())
            })?;
            if realised {
                return Ok(());
            }
            if Instant::now() >= deadline {
                // samplez-nmos turns the sender's master_enable false in
                // /active, so the output stops trying too.
                let _ = self.with_engine(|e| e.set_output_enabled(output, false, 0));
                return Err(Fail(format!(
                    "the output's flow was not created within {} s: {}",
                    CREATE_TIMEOUT.as_secs(),
                    reason.as_deref().unwrap_or("its writer did not come in")
                )));
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

impl Connections for EnginePorts {
    fn resolve(&self, port: Port, flow: Uuid) -> BoxFuture<'_, Result<FlowSeen, Reject>> {
        let seen = match port {
            Port::Receiver(i) => flow_seen(&self.inputs[i], &flow.to_string()),
            Port::Sender(_) => Ok(FlowSeen::Absent),
        };
        Box::pin(async move { seen })
    }

    /// The engine's buildLead: a route stamped later than that from now
    /// keeps its index.
    fn lead(&self) -> Duration {
        let samples = self
            .with_engine(|e| e.status().generation.build_lead)
            .unwrap_or(0);
        Duration::from_nanos(samples * 1_000_000_000 / u64::from(samplez_nmos::SAMPLE_RATE))
            + LEAD_SLACK
    }

    fn apply(
        &self,
        port: Port,
        active: Active,
        at: Option<Tai>,
        _generation: u64,
    ) -> BoxFuture<'_, Result<(), Fail>> {
        // foldz makes no external changes and applies each call before
        // the next comes, so no call is stale.
        Box::pin(async move {
            let at_index = at.map_or(0, |t| self.index(t));
            match port {
                Port::Receiver(i) => self.apply_receiver(i, &active, at_index),
                Port::Sender(o) => self.apply_sender(o, active.master_enable, at_index).await,
            }
        })
    }
}
