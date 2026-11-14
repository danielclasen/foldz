//! The foldz section of the config file: MXL domains, inputs, outputs
//! and NMOS identity. Shape, layouts and matrices are fixed at start-up.

use std::collections::HashSet;
use std::fmt;

use serde::Deserialize;

use crate::layout::Layout;
use crate::presets::{Fold, Lfe, Mapping, PresetName, fold};

/// Most member flows per output: an `MxlFlowRedundancySet` merges 2 to 7
/// members, and an unreplicated output has one.
pub const MAX_MEMBERS: usize = 7;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub nmos: Nmos,
    pub domains: Vec<Domain>,
    pub inputs: Vec<Input>,
    pub outputs: Vec<Output>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Nmos {
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
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Registry {
    /// Overrides discovery when set.
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
    /// IS-04 label of the output's Source, Flow and Sender.
    #[serde(default)]
    pub label: String,
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
    if is_lowercase_uuid(id) {
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
    // A host of digits and dots only is an IPv4 address or nothing, so
    // `999.1.1.1` or `10.1` is not taken for a DNS name.
    let host_ok = host.is_none_or(|h| {
        if h.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
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
            if !paths.insert(d.path.as_str()) {
                return err(at, format!("path {:?} is used twice", d.path));
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

    fn check_input<'a>(
        &self,
        at: &str,
        input: &'a Input,
        domains: &[&str],
        names: &mut HashSet<&'a str>,
        input_flows: &mut HashSet<&'a str>,
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
        pick(
            &format!("{at}.domain"),
            "domain",
            input.domain.as_deref(),
            domains,
        )?;
        if let Some(flow) = &input.flow {
            check_uuid(&format!("{at}.flow"), flow)?;
            if !input_flows.insert(flow.as_str()) {
                return err(
                    format!("{at}.flow"),
                    format!("flow {flow} is read by another input"),
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
        input_flows: &HashSet<&str>,
        output_flows: &mut HashSet<&'a str>,
    ) -> Result<Fold, ConfigError> {
        if output.name.is_empty() {
            return err(at, "name is empty");
        }
        if !names.insert(output.name.as_str()) {
            return err(at, "name is used twice among inputs and outputs");
        }
        let input_name = pick(
            &format!("{at}.input"),
            "input",
            output.input.as_deref(),
            input_names,
        )?;
        pick(
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
        for (at, flow) in ids.chain(merged) {
            check_uuid(&at, flow)?;
            if input_flows.contains(flow.as_str()) {
                return err(at, format!("flow {flow} is also an input flow"));
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
        if !registry.url.is_empty() && !is_http_url(&registry.url) {
            return err(
                "nmos.registry.url",
                format!("{:?} is not an http or https URL", registry.url),
            );
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
        }
        Ok(())
    }
}
