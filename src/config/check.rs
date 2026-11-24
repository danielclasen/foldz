//! Checks of a [`Config`] and its translation for the engine.

use super::*;

/// `folds`, checked to hold one fold per output of a config of `outputs`
/// outputs, in output order.
///
/// # Panics
///
/// If `folds` has another length than `outputs`, or a fold's output index
/// is not its position: a zip with the outputs would leave outputs out or
/// pair them with another output's fold.
pub(crate) fn checked(folds: &[OutputFold], outputs: usize) -> &[OutputFold] {
    let in_order = folds.iter().enumerate().all(|(i, f)| f.output == i);
    assert!(
        folds.len() == outputs && in_order,
        "{} folds for {outputs} outputs, or out of order: not validate's",
        folds.len()
    );
    folds
}

/// What [`Config::validate`] has seen so far, which each input and output
/// is checked against: the domain and input names, and the names and flow
/// ids taken by the inputs and outputs before it.
struct Seen<'a> {
    domains: Vec<&'a str>,
    input_names: Vec<&'a str>,
    /// Input and output names, which share one namespace.
    names: HashSet<&'a str>,
    /// The flows inputs read, each with its domain.
    input_flows: HashSet<(&'a str, &'a str)>,
    output_flows: HashSet<&'a str>,
}

/// A [`ConfigError`] at `at` for `reason`.
pub(crate) fn err<T>(at: impl Into<String>, reason: impl Into<String>) -> Result<T, ConfigError> {
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

/// Whether `s` is an MXL UUID: lowercase, with a version digit of 1 or
/// above and the RFC 4122 variant, the pattern of AMWA BCP-007-03.
pub fn is_mxl_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    is_lowercase_uuid(s) && b[14] != b'0' && matches!(b[19], b'8' | b'9' | b'a' | b'b')
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
fn pick(at: &str, what: &str, given: Option<&str>, names: &[&str]) -> Result<usize, ConfigError> {
    match (given, names) {
        (None, [_]) => Ok(0),
        (None, _) => err(at, format!("required with more than one {what}")),
        (Some(name), _) => match names.iter().position(|n| *n == name) {
            Some(found) => Ok(found),
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
    pub fn validate(&self) -> Result<Vec<OutputFold>, ConfigError> {
        self.check_nmos()?;
        // The machine listener serves machine clients that send commands;
        // foldz takes none.
        if self.listeners.machine.is_some() {
            return err("listeners.machine", "foldz has no machine listener");
        }
        if self.auth.machine_token.is_some() {
            return err("auth.machineToken", "foldz has no machine listener");
        }
        // samplez-service's oidc mode signs browsers in and sends them on
        // to a UI, which foldz does not serve.
        if self.auth.mode == samplez_service::config::AuthMode::Oidc {
            return err(
                "auth.mode",
                "oidc is not supported; use none, token or proxy",
            );
        }
        if self.auth.oidc.is_some() {
            return err(
                "auth.oidc",
                "oidc is not supported; use none, token or proxy",
            );
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
        let mut seen = Seen {
            domains,
            input_names: self.inputs.iter().map(|i| i.name.as_str()).collect(),
            names: HashSet::new(),
            input_flows: HashSet::new(),
            output_flows: HashSet::new(),
        };
        for (i, input) in self.inputs.iter().enumerate() {
            self.check_input(&format!("inputs[{i}]"), input, &mut seen)
                .map_err(named("input", &input.name))?;
        }

        let mut folds = Vec::with_capacity(self.outputs.len());
        for (i, output) in self.outputs.iter().enumerate() {
            let folded = self
                .check_output(&format!("outputs[{i}]"), i, output, &mut seen)
                .map_err(named("output", &output.name))?;
            folds.push(folded);
        }
        if self.nmos.enabled {
            self.check_nmos_model()?;
        }
        Ok(folds)
    }

    /// What the NMOS node model takes beyond the rest of the config: MXL
    /// UUIDs (BCP-007-03) for every flow and domain id, and one Sender per
    /// grouphint group.
    fn check_nmos_model(&self) -> Result<(), ConfigError> {
        let mxl = |at: String, id: &str| {
            if is_mxl_uuid(id) {
                Ok(())
            } else {
                err(
                    at,
                    format!(
                        "{id} is not an MXL UUID (version digit 1 to f, variant 8 to b), which NMOS needs"
                    ),
                )
            }
        };
        for (i, d) in self.domains.iter().enumerate() {
            if let Some(id) = &d.id {
                mxl(format!("domains[{i}].id"), id)?;
            }
        }
        for (i, input) in self.inputs.iter().enumerate() {
            if let Some(flow) = &input.flow {
                mxl(format!("inputs[{i}].flow"), flow)?;
            }
        }
        let mut groups = HashSet::new();
        for (i, output) in self.outputs.iter().enumerate() {
            mxl(format!("outputs[{i}].flows[0]"), &output.flows[0])?;
            if !groups.insert(output.group()) {
                return err(
                    format!("outputs[{i}].group"),
                    format!(
                        "group {:?} is another output's; NMOS gives each group one Sender",
                        output.group()
                    ),
                );
            }
        }
        Ok(())
    }

    /// Reads, parses and checks the config file at `path`, and returns it
    /// with each output's fold.
    pub fn load(path: &Path) -> Result<(Config, Vec<OutputFold>), ConfigError> {
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
    ///
    /// # Panics
    ///
    /// If the config did not pass [`Config::validate`].
    pub fn engine(&self, folds: &[OutputFold]) -> Result<samplez::Config, ConfigError> {
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
        for (output, f) in self.outputs.iter().zip(checked(folds, self.outputs.len())) {
            let first = first_row[f.input];
            for j in 0..f.matrix.outputs() {
                for i in 0..f.matrix.inputs() {
                    matrix[first + i][column + j] = f.matrix.gain(j, i);
                }
            }
            column += f.matrix.outputs();
            outputs.push(serde_json::json!({
                "name": output.name,
                "domain": self.domains[f.domain].name,
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
        seen: &mut Seen<'a>,
    ) -> Result<(), ConfigError> {
        if input.name.is_empty() {
            return err(at, "name is empty");
        }
        if !seen.names.insert(input.name.as_str()) {
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
        let domain = seen.domains[pick(
            &format!("{at}.domain"),
            "domain",
            input.domain.as_deref(),
            &seen.domains,
        )?];
        if let Some(flow) = &input.flow {
            check_uuid(&format!("{at}.flow"), flow)?;
            // A flow id in another domain is another flow.
            if !seen.input_flows.insert((domain, flow.as_str())) {
                return err(
                    format!("{at}.flow"),
                    format!("flow {flow} is read by another input in domain {domain:?}"),
                );
            }
        }
        Ok(())
    }

    fn check_output<'a>(
        &self,
        at: &str,
        index: usize,
        output: &'a Output,
        seen: &mut Seen<'a>,
    ) -> Result<OutputFold, ConfigError> {
        if output.name.is_empty() {
            return err(at, "name is empty");
        }
        if !seen.names.insert(output.name.as_str()) {
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
        let input_index = pick(
            &format!("{at}.input"),
            "input",
            output.input.as_deref(),
            &seen.input_names,
        )?;
        let input_name = seen.input_names[input_index];
        let domain_index = pick(
            &format!("{at}.domain"),
            "domain",
            output.domain.as_deref(),
            &seen.domains,
        )?;
        let domain = seen.domains[domain_index];

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
            if seen.input_flows.contains(&(domain, flow.as_str())) {
                return err(
                    at,
                    format!("flow {flow} is also an input flow in domain {domain:?}"),
                );
            }
            if !seen.output_flows.insert(flow.as_str()) {
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
        .map(|fold| OutputFold {
            fold,
            output: index,
            input: input_index,
            domain: domain_index,
        })
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
            // The registration client speaks plain HTTP and appends the
            // API path itself.
            let base = registry.url.strip_suffix('/').unwrap_or(&registry.url);
            let plain = base
                .strip_prefix("http://")
                .is_some_and(|rest| !rest.contains('/'));
            if !registry.url.is_empty() && !plain {
                return err(
                    "nmos.registry.url",
                    format!(
                        "{:?} is not an http://host[:port] URL, which NMOS needs",
                        registry.url
                    ),
                );
            }
        }
        Ok(())
    }
}
