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

fn err<T>(at: impl Into<String>, reason: impl Into<String>) -> Result<T, ConfigError> {
    Err(ConfigError {
        at: at.into(),
        reason: reason.into(),
    })
}

/// Whether `s` is a UUID in the canonical textual form of MXL's
/// Addressability.md, in lowercase: IS-04 resource ids and BCP-007-03 flow
/// ids are lowercase.
pub fn is_lowercase_uuid(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(b),
        })
}

fn check_uuid(at: &str, id: &str) -> Result<(), ConfigError> {
    if id == UNCONNECTED_FLOW {
        err(at, "the nil UUID is reserved")
    } else if is_lowercase_uuid(id) {
        Ok(())
    } else {
        err(
            at,
            format!("{id:?} is not a lowercase UUID (xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx)"),
        )
    }
}

/// Resolves an optional reference to a named item, required when there is
/// more than one item.
fn pick<'a>(
    at: &str,
    what: &str,
    given: Option<&str>,
    names: &[&'a str],
) -> Result<&'a str, ConfigError> {
    match (given, names) {
        (None, [only]) => Ok(*only),
        (None, _) => err(at, format!("required with more than one {what}")),
        (Some(name), _) => match names.iter().find(|n| **n == name) {
            Some(found) => Ok(*found),
            None => err(at, format!("no {what} named {name:?}")),
        },
    }
}

/// Whether `s` is a DNS name: dot-separated labels of 1 to 63 ASCII
/// letters, digits and hyphens, no label starting or ending with a hyphen,
/// at most 253 characters without an optional trailing dot.
pub fn is_dns_name(s: &str) -> bool {
    let s = s.strip_suffix('.').unwrap_or(s);
    !s.is_empty()
        && s.len() <= 253
        && s.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

/// Whether `s` is an `http` or `https` URL whose host is a DNS name, an
/// IPv4 address or a bracketed IPv6 address, with an optional port of 1
/// to 65535 and an optional path of printable ASCII without spaces.
pub fn is_http_url(s: &str) -> bool {
    let Some(rest) = s
        .strip_prefix("http://")
        .or_else(|| s.strip_prefix("https://"))
    else {
        return false;
    };
    let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let (host, port) = match authority.strip_prefix('[') {
        Some(v6) => match v6.split_once(']') {
            Some((addr, port)) if addr.parse::<std::net::Ipv6Addr>().is_ok() => (None, port),
            _ => return false,
        },
        None => {
            let (host, port) = authority.split_at(authority.find(':').unwrap_or(authority.len()));
            (Some(host), port)
        }
    };
    let port_ok = match port.strip_prefix(':') {
        Some(p) => p.bytes().all(|b| b.is_ascii_digit()) && p.parse::<u16>().is_ok_and(|p| p > 0),
        None => port.is_empty(),
    };
    // A WHATWG URL parser takes a host whose last label is a decimal or
    // `0x` number for IPv4, so such a host is a dotted-decimal IPv4
    // address or nothing: `999.1.1.1`, `10.1`, `registry.10` and
    // `0x7f.0.0.1` are not taken for DNS names.
    let host_ok = host.is_none_or(|h| {
        let last = h.strip_suffix('.').unwrap_or(h).rsplit('.').next();
        let numeric = last.is_some_and(|l| {
            let hex = l.strip_prefix("0x").or_else(|| l.strip_prefix("0X"));
            match hex {
                Some(digits) => digits.bytes().all(|b| b.is_ascii_hexdigit()),
                None => !l.is_empty() && l.bytes().all(|b| b.is_ascii_digit()),
            }
        });
        if numeric {
            h.parse::<std::net::Ipv4Addr>().is_ok()
        } else {
            is_dns_name(h)
        }
    });
    port_ok && host_ok && path.bytes().all(|b| b.is_ascii_graphic())
}

/// Adds which `kind` named `name` an error concerns to its reason.
fn named(kind: &'static str, name: &str) -> impl FnOnce(ConfigError) -> ConfigError {
    move |e| ConfigError {
        at: e.at,
        reason: format!("{kind} {name:?}: {}", e.reason),
    }
}

impl Config {
    /// Checks the whole config and returns each output's fold, in output
    /// order.
    pub fn validate(&self) -> Result<Vec<Fold>, ConfigError> {
        self.check_nmos()?;
        // The machine listener serves machine clients that send commands;
        // foldz takes none.
        if self.listeners.machine.is_some() {
            return err("listeners.machine", "foldz has no machine listener");
        }
        if self.auth.machine_token.is_some() {
            return err("auth.machineToken", "foldz has no machine listener");
        }

        if self.domains.is_empty() {
            return err("domains", "at least one domain is required");
        }
        let mut paths = HashSet::new();
        let mut domains = Vec::new();
        for (i, d) in self.domains.iter().enumerate() {
            let at = format!("domains[{i}]");
            if d.name.is_empty() {
                return err(at, "name is empty");
            }
            if domains.contains(&d.name.as_str()) {
                return err(at, format!("domain name {:?} is used twice", d.name));
            }
            if !d.path.starts_with('/') {
                return err(at, format!("path {:?} is not absolute", d.path));
            }
            let normal: PathBuf = Path::new(&d.path).components().collect();
            if normal.as_os_str() != d.path.as_str()
                || normal
                    .components()
                    .any(|c| matches!(c, Component::ParentDir))
            {
                return err(
                    at,
                    format!(
                        "path {:?} has a trailing or repeated '/' or a '.' or '..' segment",
                        d.path
                    ),
                );
            }
            if !paths.insert(normal) {
                return err(at, format!("path {:?} is used twice", d.path));
            }
            if let Some(id) = &d.id {
                check_uuid(&format!("{at}.id"), id)?;
            }
            domains.push(d.name.as_str());
        }

        if self.inputs.is_empty() {
            return err("inputs", "at least one input is required");
        }
        if self.outputs.is_empty() {
            return err("outputs", "at least one output is required");
        }
        let mut names = HashSet::new();
        let mut input_flows = HashSet::new();
        for (i, input) in self.inputs.iter().enumerate() {
            self.check_input(
                &format!("inputs[{i}]"),
                input,
                &domains,
                &mut names,
                &mut input_flows,
            )
            .map_err(named("input", &input.name))?;
        }

        let input_names: Vec<&str> = self.inputs.iter().map(|i| i.name.as_str()).collect();
        let mut output_flows = HashSet::new();
        let mut folds = Vec::with_capacity(self.outputs.len());
        for (i, output) in self.outputs.iter().enumerate() {
            let folded = self
                .check_output(
                    &format!("outputs[{i}]"),
                    output,
                    &domains,
                    &input_names,
                    &mut names,
                    &input_flows,
                    &mut output_flows,
                )
                .map_err(named("output", &output.name))?;
            folds.push(folded);
        }
        Ok(folds)
    }

    /// Reads, parses and checks the config file at `path`, and returns it
    /// with each output's fold.
    pub fn load(path: &Path) -> Result<(Config, Vec<Fold>), ConfigError> {
        let at = path.display().to_string();
        let text = std::fs::read_to_string(path).or_else(|e| err(&at, e.to_string()))?;
        let config: Config = serde_json::from_str(&text).or_else(|e| err(&at, e.to_string()))?;
        let folds = config.validate()?;
        Ok((config, folds))
    }

    /// The samplez engine config of this foldz config and its `folds`, from
    /// [`Config::validate`]: one engine input per input, one engine output
    /// per output, and one gain matrix holding every output's fold, rows
    /// per input channel and columns per output channel. An input without
    /// a flow starts on [`UNCONNECTED_FLOW`], to be detached at once; an
    /// output whose input reads nothing is written silent.
    ///
    /// Refuses what this build does not implement: more than one member
    /// flow per output, which needs replication.
    pub fn engine(&self, folds: &[Fold]) -> Result<samplez::Config, ConfigError> {
        if self.outputs.iter().any(|o| o.flows.len() > 1) {
            return err(
                "outputs",
                "more than one member flow per output needs replication, which is not \
                 implemented in this build",
            );
        }
        let only_domain = (self.domains.len() == 1).then(|| self.domains[0].name.as_str());
        let domain = |given: &Option<String>| {
            given
                .as_deref()
                .or(only_domain)
                .expect("validate resolved the domain")
                .to_owned()
        };
        // The first engine matrix row of each input.
        let mut first_row = Vec::with_capacity(self.inputs.len());
        let mut rows = 0;
        let mut inputs = Vec::new();
        for input in &self.inputs {
            first_row.push(rows);
            rows += input.channels;
            inputs.push(serde_json::json!({
                "name": input.name,
                "domain": domain(&input.domain),
                "flowId": input.flow.as_deref().unwrap_or(UNCONNECTED_FLOW),
                "channels": input.channels,
            }));
        }
        let columns: usize = folds.iter().map(|f| f.matrix.outputs()).sum();
        let mut matrix = vec![vec![0.0f32; columns]; rows];
        let mut outputs = Vec::new();
        let mut column = 0;
        for (output, f) in self.outputs.iter().zip(folds) {
            let input = match &output.input {
                Some(name) => self.inputs.iter().position(|i| &i.name == name),
                None => Some(0),
            }
            .expect("validate resolved the input");
            let first = first_row[input];
            for j in 0..f.matrix.outputs() {
                for i in 0..f.matrix.inputs() {
                    matrix[first + i][column + j] = f.matrix.gain(j, i);
                }
            }
            column += f.matrix.outputs();
            outputs.push(serde_json::json!({
                "name": output.name,
                "domain": domain(&output.domain),
                "flowId": output.flows[0],
                "label": output.label(),
                "group": output.group(),
                "role": output.role(),
                "channels": f.matrix.outputs(),
            }));
        }
        let e = &self.engine;
        let rt = &self.rt;
        let rt_cpu = match rt.rt_cpu {
            samplez::config::RtCpu::Auto => serde_json::json!("auto"),
            samplez::config::RtCpu::None => serde_json::json!("none"),
            samplez::config::RtCpu::First => serde_json::json!("first"),
            samplez::config::RtCpu::Last => serde_json::json!("last"),
            samplez::config::RtCpu::Index(i) => serde_json::json!(i),
        };
        let v = serde_json::json!({
            "sampleRate": e.sample_rate,
            "blockSize": e.block_size,
            "readerMarginSamples": e.reader_margin_samples,
            "maxCatchupBlocks": e.max_catchup_blocks,
            "pruneAfterBlocks": e.prune_after_blocks,
            "rampSamples": e.ramp_samples,
            "libmxl": self.mxl.library_path,
            "domains": self.domains.iter().map(|d| serde_json::json!({
                "name": d.name, "path": d.path,
            })).collect::<Vec<_>>(),
            "inputs": inputs,
            "outputs": outputs,
            "matrix": matrix,
            "rt": { "enabled": rt.enabled, "priority": rt.priority, "rtCpu": rt_cpu },
        });
        samplez::Config::parse(&v.to_string()).or_else(|e| {
            let reason = e.0;
            err("engine", reason)
        })
    }

    fn check_input<'a>(
        &self,
        at: &str,
        input: &'a Input,
        domains: &[&'a str],
        names: &mut HashSet<&'a str>,
        input_flows: &mut HashSet<(&'a str, &'a str)>,
    ) -> Result<(), ConfigError> {
        if input.name.is_empty() {
            return err(at, "name is empty");
        }
        if !names.insert(input.name.as_str()) {
            return err(at, "name is used twice");
        }
        if input.channels != input.layout.len() {
            return err(
                format!("{at}.layout"),
                format!(
                    "has {} labels for {} channels",
                    input.layout.len(),
                    input.channels
                ),
            );
        }
        let domain = pick(
            &format!("{at}.domain"),
            "domain",
            input.domain.as_deref(),
            domains,
        )?;
        if let Some(flow) = &input.flow {
            check_uuid(&format!("{at}.flow"), flow)?;
            // A flow id in another domain is another flow.
            if !input_flows.insert((domain, flow.as_str())) {
                return err(
                    format!("{at}.flow"),
                    format!("flow {flow} is read by another input in domain {domain:?}"),
                );
            }
        }
        Ok(())
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the sets of names and flow ids seen so far are threaded through every output"
    )]
    fn check_output<'a>(
        &self,
        at: &str,
        output: &'a Output,
        domains: &[&str],
        input_names: &[&str],
        names: &mut HashSet<&'a str>,
        input_flows: &HashSet<(&str, &str)>,
        output_flows: &mut HashSet<&'a str>,
    ) -> Result<Fold, ConfigError> {
        if output.name.is_empty() {
            return err(at, "name is empty");
        }
        if !names.insert(output.name.as_str()) {
            return err(at, "name is used twice among inputs and outputs");
        }
        for (field, value) in [("group", output.group()), ("role", output.role())] {
            if value.is_empty() || value.contains(':') {
                return err(
                    format!("{at}.{field}"),
                    format!("{value:?} is empty or holds ':', the grouphint separator"),
                );
            }
        }
        let input_name = pick(
            &format!("{at}.input"),
            "input",
            output.input.as_deref(),
            input_names,
        )?;
        let domain = pick(
            &format!("{at}.domain"),
            "domain",
            output.domain.as_deref(),
            domains,
        )?;

        let members = self.outputs[0].flows.len();
        let flows_at = format!("{at}.flows");
        if !(1..=MAX_MEMBERS).contains(&output.flows.len()) {
            return err(
                flows_at,
                format!(
                    "has {} flow ids, 1 to {MAX_MEMBERS} are allowed",
                    output.flows.len()
                ),
            );
        }
        if output.flows.len() != members {
            return err(
                flows_at,
                format!(
                    "has {} flow ids and outputs[0] {members}; every replica writes one member \
                     flow per output",
                    output.flows.len()
                ),
            );
        }
        let merged_at = format!("{at}.mergedFlow");
        let ids = output
            .flows
            .iter()
            .enumerate()
            .map(|(o, f)| (format!("{flows_at}[{o}]"), f));
        let merged = match (&output.merged_flow, members) {
            (None, 1) => None,
            (Some(_), 1) => return err(merged_at, "allowed with more than one member flow only"),
            (None, _) => return err(merged_at, "required with more than one member flow"),
            (Some(id), _) => Some((merged_at, id)),
        };
        // Written ids are unique across domains, as samplez requires of
        // its outputs; an id read in another domain is another flow.
        for (at, flow) in ids.chain(merged) {
            check_uuid(&at, flow)?;
            if input_flows.contains(&(domain, flow.as_str())) {
                return err(
                    at,
                    format!("flow {flow} is also an input flow in domain {domain:?}"),
                );
            }
            if !output_flows.insert(flow.as_str()) {
                return err(at, format!("flow {flow} is used twice"));
            }
        }

        if output.preset.is_some() && output.matrix.is_some() {
            return err(at, "preset and matrix are mutually exclusive");
        }
        let mapping = match (&output.preset, &output.source, &output.matrix) {
            (Some(PresetName::Shuffle), Some(source), _) => Mapping::Shuffle(source),
            (Some(PresetName::Shuffle), None, _) => {
                return err(format!("{at}.source"), "required with preset shuffle");
            }
            (_, Some(_), _) => {
                return err(format!("{at}.source"), "allowed with preset shuffle only");
            }
            (Some(PresetName::Select), None, _) => Mapping::Select,
            (Some(PresetName::Table(preset)), None, _) => Mapping::Preset(*preset),
            (None, None, Some(rows)) => Mapping::Matrix(rows),
            (None, None, None) => return err(at, "preset or matrix is required"),
        };
        let input = self
            .inputs
            .iter()
            .find(|i| i.name == input_name)
            .expect("pick returned a configured input");
        fold(
            mapping,
            &input.layout,
            &output.layout,
            output.lfe,
            output.gain_db,
        )
        .or_else(|e| err(at, e.to_string()))
    }

    fn check_nmos(&self) -> Result<(), ConfigError> {
        let nmos = &self.nmos;
        if let Some(seed) = &nmos.seed {
            check_uuid("nmos.seed", seed)?;
            if !nmos.seed_name.is_empty() {
                return err("nmos", "seed and seedName are mutually exclusive");
            }
        }
        let registry = &nmos.registry;
        // The registration client speaks plain HTTP and appends the API
        // path itself.
        let base = registry.url.strip_suffix('/').unwrap_or(&registry.url);
        let plain = base
            .strip_prefix("http://")
            .is_some_and(|rest| !rest.contains('/'));
        let url_ok = registry.url.is_empty() || plain && is_http_url(base);
        if !url_ok {
            return err(
                "nmos.registry.url",
                format!("{:?} is not an http://host[:port] URL", registry.url),
            );
        }
        if !(1..=60).contains(&nmos.heartbeat_s) {
            return err("nmos.heartbeatS", "1 to 60 seconds are allowed");
        }
        if nmos.asset.manufacturer.is_empty() {
            return err("nmos.asset.manufacturer", "is empty");
        }
        if !registry.dns_sd_domain.is_empty() && !is_dns_name(&registry.dns_sd_domain) {
            return err(
                "nmos.registry.dnsSdDomain",
                format!("{:?} is not a DNS name", registry.dns_sd_domain),
            );
        }
        if nmos.enabled {
            if nmos.seed.is_none() && nmos.seed_name.is_empty() {
                return err("nmos", "enabled needs seed or seedName");
            }
            if nmos.advertise_host.is_empty() {
                return err("nmos.advertiseHost", "required when NMOS is enabled");
            }
            let listener = &self.listeners.nmos;
            if nmos.advertise_port == 0 && listener.port == 0 {
                return err(
                    "nmos.advertisePort",
                    "required when listeners.nmos.port is 0",
                );
            }
            if listener.tls.mode != samplez_service::config::TlsMode::None {
                return err(
                    "listeners.nmos.tls",
                    "the Node advertises its APIs over plain HTTP",
                );
            }
        }
        Ok(())
    }
}
