//! The NMOS node model, the domain ids and the flow definitions foldz
//! reads; the node itself runs in the e2e test and the nmos-testing suites.

use std::path::PathBuf;

use foldz::config::{Config, Domain};
use foldz::nmos::{domain_ids, model, nmos_config, parse_flow_def};
use samplez_nmos::{ChannelCount, FlowSeen};
use serde_json::json;

const DOMAIN: &str = "5e0c2a1b-7d3f-4e9a-8b6c-1f2e3d4c5b6a";
const OTHER: &str = "5e0c2a1b-7d3f-4e9a-8b6c-1f2e3d4c5b6b";

/// A fresh directory under the target's temporary directory.
fn dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("nmos-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn domain(path: &std::path::Path, id: Option<&str>) -> Domain {
    Domain {
        name: "d".into(),
        path: path.display().to_string(),
        id: id.map(Into::into),
    }
}

#[test]
fn a_domain_id_comes_from_domain_def_json_or_the_config() {
    let with_file = dir("with-file");
    std::fs::write(
        with_file.join("domain_def.json"),
        json!({ "id": DOMAIN, "label": "test", "description": "", "tags": {} }).to_string(),
    )
    .unwrap();
    let without = dir("without-file");

    let ids = domain_ids(&[domain(&with_file, None)]).unwrap();
    assert_eq!(ids[0].to_string(), DOMAIN);
    let ids = domain_ids(&[domain(&with_file, Some(DOMAIN))]).unwrap();
    assert_eq!(ids[0].to_string(), DOMAIN);
    let ids = domain_ids(&[domain(&without, Some(OTHER))]).unwrap();
    assert_eq!(ids[0].to_string(), OTHER);

    let e = domain_ids(&[domain(&with_file, Some(OTHER))]).unwrap_err();
    assert_eq!(e.at, "domains[0].id");
    assert!(e.reason.contains("differs"), "{e}");
    let e = domain_ids(&[domain(&without, None)]).unwrap_err();
    assert_eq!(e.at, "domains[0].id");
    assert!(e.reason.contains("does not exist"), "{e}");

    let bad = dir("bad-file");
    std::fs::write(bad.join("domain_def.json"), r#"{"id": "NOT-A-UUID"}"#).unwrap();
    let e = domain_ids(&[domain(&bad, Some(DOMAIN))]).unwrap_err();
    assert!(e.at.ends_with("domain_def.json"), "{e}");
}

#[test]
fn flow_definitions_give_channels_rate_and_format() {
    let audio = |media: &str, rate: serde_json::Value| {
        json!({
            "id": DOMAIN, "format": "urn:x-nmos:format:audio", "media_type": media,
            "sample_rate": rate, "channel_count": 6, "bit_depth": 32,
        })
        .to_string()
    };
    assert_eq!(
        parse_flow_def(&audio("audio/float32", json!({ "numerator": 48000 }))),
        Some(FlowSeen::Present {
            channels: 6,
            rate: 48_000,
            float32: true
        })
    );
    assert_eq!(
        parse_flow_def(&audio(
            "audio/L24",
            json!({ "numerator": 96000, "denominator": 2 })
        )),
        Some(FlowSeen::Present {
            channels: 6,
            rate: 48_000,
            float32: false
        })
    );
    let video = json!({ "format": "urn:x-nmos:format:video", "media_type": "video/v210" });
    assert_eq!(parse_flow_def(&video.to_string()), None);
    assert_eq!(
        parse_flow_def(&audio(
            "audio/float32",
            json!({ "numerator": 1, "denominator": 0 })
        )),
        None
    );
    assert_eq!(parse_flow_def("not json"), None);
}

fn config() -> Config {
    serde_json::from_value(json!({
        "nmos": {
            "enabled": true, "seedName": "ns/foldz", "advertiseHost": "foldz.example",
            "registry": { "url": "http://registry:8010/" }
        },
        "listeners": { "nmos": { "bind": "0.0.0.0", "port": 18081 } },
        "domains": [{ "name": "a", "path": "/mxl/a" }, { "name": "b", "path": "/mxl/b" }],
        "inputs": [
            { "name": "main", "domain": "b", "channels": 6,
              "layout": ["L", "R", "C", "LFE", "Ls", "Rs"],
              "flow": "7a1e4c9b-3d2f-4b6a-8e0c-5f7d9b1a3c55" },
            { "name": "spare", "domain": "a", "channels": 2, "layout": ["L", "-"] }
        ],
        "outputs": [
            { "name": "stereo", "input": "main", "domain": "a", "label": "Programme",
              "flows": ["0f3a6c2e-5b1d-4e8a-9c7f-2d4b6a8e0c11"], "layout": ["L", "R"],
              "preset": "bs775-3_2-to-2_0" }
        ]
    }))
    .unwrap()
}

#[test]
fn every_input_is_a_receiver_and_every_output_a_sender() {
    let config = config();
    config.validate().unwrap();
    let a: samplez_nmos::uuid::Uuid = DOMAIN.parse().unwrap();
    let b: samplez_nmos::uuid::Uuid = OTHER.parse().unwrap();
    let m = model(&config, &[a, b]);

    assert_eq!(m.senders.len(), 1);
    let s = &m.senders[0];
    assert_eq!((s.key.as_str(), s.label.as_str()), ("stereo", "Programme"));
    assert_eq!(s.group, "stereo");
    assert_eq!(s.domain, a);
    assert_eq!(s.flow.to_string(), "0f3a6c2e-5b1d-4e8a-9c7f-2d4b6a8e0c11");
    let symbols: Vec<_> = s.channels.iter().map(|c| c.symbol.as_deref()).collect();
    assert_eq!(symbols, [Some("L"), Some("R")]);

    let keys: Vec<_> = m.receivers.iter().map(|r| r.key.as_str()).collect();
    assert_eq!(keys, ["main", "spare"]);
    let main = &m.receivers[0];
    assert_eq!(main.channels, ChannelCount::Exactly(6));
    assert_eq!(main.domain, b);
    assert_eq!(
        main.flow.map(|f| f.to_string()).as_deref(),
        Some("7a1e4c9b-3d2f-4b6a-8e0c-5f7d9b1a3c55")
    );
    let spare = &m.receivers[1];
    assert_eq!((spare.domain, spare.flow), (a, None));

    let c = nmos_config(&config);
    assert_eq!(c.seed, "ns/foldz");
    assert_eq!(c.asset.instance_id, "ns/foldz");
    assert_eq!(c.asset.manufacturer, "danielclasen");
    assert_eq!(
        (c.asset.product.as_str(), c.asset.functions.as_slice()),
        ("foldz", ["Downmix".to_owned()].as_slice())
    );
    // advertisePort 0: the listener's port.
    assert_eq!(c.advertise_port, 18081);
    assert_eq!(c.registries, ["http://registry:8010"]);
    assert_eq!(c.dns_sd_domain, None);
    assert_eq!(c.heartbeat, std::time::Duration::from_secs(5));
}
