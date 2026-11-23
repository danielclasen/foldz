//! The config file: the engine, RT and listener settings, MXL domains,
//! inputs, outputs and NMOS identity. Shape, layouts and matrices are fixed
//! at start-up.

use std::collections::HashSet;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use samplez_service::config::{ApiConfig, AuthConfig, Listeners};
use serde::Deserialize;

use crate::layout::Layout;
use crate::presets::{Fold, Lfe, Mapping, PresetName, fold};

mod check;

pub(crate) use check::err;
pub use check::{is_dns_name, is_http_url, is_lowercase_uuid, is_mxl_uuid};

/// Most member flows per output: an `MxlFlowRedundancySet` merges 2 to 7
/// members, and an unreplicated output has one.
pub const MAX_MEMBERS: usize = 7;

/// The flow an unconnected input's engine slot is started on and detached
/// from at once: samplez starts every slot on a flow. The nil UUID is no
/// MXL flow id (BCP-007-03 requires a version digit of 1 or above), so no
/// writer produces it, and the config refuses it as a flow id.
pub const UNCONNECTED_FLOW: &str = "00000000-0000-0000-0000-000000000000";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub mxl: Mxl,
    #[serde(default)]
    pub engine: Engine,
    #[serde(default)]
    pub rt: samplez::config::Rt,
    /// The API, machine, ops and NMOS listeners and their TLS.
    #[serde(default)]
    pub listeners: Listeners,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub api: ApiConfig,
    #[serde(default)]
    pub nmos: Nmos,
    pub domains: Vec<Domain>,
    pub inputs: Vec<Input>,
    pub outputs: Vec<Output>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Mxl {
    /// The libmxl shared library, loaded at start.
    #[serde(default = "default_library_path")]
    pub library_path: PathBuf,
}

impl Default for Mxl {
    fn default() -> Self {
        Mxl {
            library_path: default_library_path(),
        }
    }
}

fn default_library_path() -> PathBuf {
    PathBuf::from("/opt/mxl/lib/libmxl.so")
}

/// The samplez engine's timing; samplez checks the values.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Engine {
    /// 48000 is the only rate samplez runs at.
    #[serde(default = "default_sample_rate")]
    pub sample_rate: u32,
    /// Block size B in samples, 16 to 1024.
    #[serde(default = "default_block_size")]
    pub block_size: u32,
    /// The read delay behind libmxl's clock.
    #[serde(default = "default_margin")]
    pub reader_margin_samples: u32,
    #[serde(default = "default_catchup")]
    pub max_catchup_blocks: u32,
    #[serde(default = "default_prune")]
    pub prune_after_blocks: u32,
    /// The length of an input's fade in and out.
    #[serde(default = "default_ramp")]
    pub ramp_samples: u32,
}

impl Default for Engine {
    fn default() -> Self {
        Engine {
            sample_rate: default_sample_rate(),
            block_size: default_block_size(),
            reader_margin_samples: default_margin(),
            max_catchup_blocks: default_catchup(),
            prune_after_blocks: default_prune(),
            ramp_samples: default_ramp(),
        }
    }
}

// The samplez defaults (samplez src/config.rs at v0.1.0).
fn default_sample_rate() -> u32 {
    48_000
}
fn default_block_size() -> u32 {
    48
}
fn default_margin() -> u32 {
    576
}
fn default_catchup() -> u32 {
    2
}
fn default_prune() -> u32 {
    50
}
fn default_ramp() -> u32 {
    480
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Nmos {
    /// Registers the inputs and outputs as NMOS receivers and senders and
    /// takes IS-05 connections on the NMOS listener.
    #[serde(default)]
    pub enabled: bool,
    /// Seed of the UUIDv5 NMOS resource ids.
    #[serde(default)]
    pub seed: Option<String>,
    /// Name the seed is derived from when `seed` is absent.
    #[serde(default)]
    pub seed_name: String,
    #[serde(default)]
    pub advertise_host: String,
    /// 0 advertises the NMOS listener's own port.
    #[serde(default)]
    pub advertise_port: u16,
    #[serde(default)]
    pub registry: Registry,
    #[serde(default)]
    pub asset: Asset,
    /// Seconds between heartbeats to the registry, and the timeout of each
    /// request to it; 1 to 60.
    #[serde(default = "default_heartbeat")]
    pub heartbeat_s: u64,
}

impl Default for Nmos {
    fn default() -> Self {
        Nmos {
            enabled: false,
            seed: None,
            seed_name: String::new(),
            advertise_host: String::new(),
            advertise_port: 0,
            registry: Registry::default(),
            asset: Asset::default(),
            heartbeat_s: default_heartbeat(),
        }
    }
}

impl Nmos {
    /// The string the NMOS resource ids derive from: `seed`, else
    /// `seedName`.
    pub fn seed(&self) -> &str {
        self.seed.as_deref().unwrap_or(&self.seed_name)
    }
}

fn default_heartbeat() -> u64 {
    5
}

/// BCP-002-02 asset tags of the Node and Device.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Asset {
    #[serde(default = "default_manufacturer")]
    pub manufacturer: String,
}

impl Default for Asset {
    fn default() -> Self {
        Asset {
            manufacturer: default_manufacturer(),
        }
    }
}

fn default_manufacturer() -> String {
    "danielclasen".to_owned()
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Registry {
    /// `http://host[:port]` of a Registration API; overrides discovery
    /// when set.
    #[serde(default)]
    pub url: String,
    /// Domain browsed by unicast DNS-SD.
    #[serde(default)]
    pub dns_sd_domain: String,
}

/// A mounted MXL domain.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Domain {
    pub name: String,
    pub path: String,
    /// The domain's MXL id, for NMOS; the domain's `domain_def.json`
    /// gives it when present, and both present must agree.
    #[serde(default)]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Input {
    pub name: String,
    pub channels: usize,
    pub layout: Layout,
    /// Required with more than one domain.
    #[serde(default)]
    pub domain: Option<String>,
    /// The MXL flow read; absent or null leaves the input unconnected.
    #[serde(default)]
    pub flow: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Output {
    pub name: String,
    /// The input read; required with more than one input.
    #[serde(default)]
    pub input: Option<String>,
    /// Required with more than one domain.
    #[serde(default)]
    pub domain: Option<String>,
    /// MXL flow id written by each replica, indexed by replica ordinal.
    pub flows: Vec<String>,
    /// Id of the flow the `MxlFlowRedundancySet` merges the members into;
    /// required with more than one member, refused with one.
    #[serde(default)]
    pub merged_flow: Option<String>,
    /// IS-04 label of the output's Source, Flow and Sender; the output's
    /// name when empty.
    #[serde(default)]
    pub label: String,
    /// The group name of the flow's `urn:x-nmos:tag:grouphint/v1.0` tag,
    /// `<group>:<role>`; the output's name when absent.
    #[serde(default)]
    pub group: Option<String>,
    /// The role in that group; `Audio` when absent.
    #[serde(default)]
    pub role: Option<String>,
    pub layout: Layout,
    /// Exactly one of `preset` and `matrix`.
    #[serde(default)]
    pub preset: Option<PresetName>,
    /// With `preset: shuffle`: per output channel, the 0-based input
    /// channel it copies.
    #[serde(default)]
    pub source: Option<Vec<usize>>,
    /// Rows per output channel, columns per input channel.
    #[serde(default)]
    pub matrix: Option<Vec<Vec<f64>>>,
    #[serde(default)]
    pub lfe: Option<Lfe>,
    #[serde(default)]
    pub gain_db: f64,
}

impl Output {
    /// The IS-04 label: `label`, or the output's name when that is empty.
    pub fn label(&self) -> &str {
        if self.label.is_empty() {
            &self.name
        } else {
            &self.label
        }
    }

    pub fn group(&self) -> &str {
        self.group.as_deref().unwrap_or(&self.name)
    }

    pub fn role(&self) -> &str {
        self.role.as_deref().unwrap_or("Audio")
    }
}

/// A config the binary refuses to start with: where, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    pub at: String,
    pub reason: String,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.at, self.reason)
    }
}

impl std::error::Error for ConfigError {}
