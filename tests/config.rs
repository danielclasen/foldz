//! Config parsing and validation.

use foldz::config::{Config, ConfigError, is_lowercase_uuid};
use serde_json::{Value, json};

const OUT_A: &str = "0f3a6c2e-5b1d-4e8a-9c7f-2d4b6a8e0c11";
const OUT_B: &str = "0f3a6c2e-5b1d-4e8a-9c7f-2d4b6a8e0c12";
const IN_A: &str = "7a1e4c9b-3d2f-4b6a-8e0c-5f7d9b1a3c55";

fn base() -> Value {
    json!({
        "domains": [{ "name": "a", "path": "/mxl/a" }],
        "inputs": [{
            "name": "main",
            "channels": 8,
            "layout": ["L", "R", "C", "LFE", "Ls", "Rs", "-", "-"],
            "flow": IN_A
        }],
        "outputs": [{
            "name": "stereo",
            "flows": [OUT_A],
            "label": "Programme stereo",
            "layout": ["L", "R"],
            "preset": "bs775-3_2-to-2_0",
            "lfe": "drop",
            "gainDb": 0
        }]
    })
}

fn parse(v: Value) -> Result<Config, serde_json::Error> {
    serde_json::from_value(v)
}

fn invalid(v: Value) -> ConfigError {
    parse(v).unwrap().validate().unwrap_err()
}

#[track_caller]
fn assert_invalid(v: Value, at: &str, reason: &str) {
    let e = invalid(v);
    assert_eq!(e.at, at, "{e}");
    assert!(e.reason.contains(reason), "{e}");
}

#[test]
fn minimal_config_is_valid() {
    let folds = parse(base()).unwrap().validate().unwrap();
    assert_eq!(folds.len(), 1);
    assert_eq!(folds[0].consumed, [0, 1, 2, 4, 5]);
}

#[test]
fn every_section_accepts_its_fields() {
    let mut v = base();
    v["nmos"] = json!({
        "enabled": true,
        "seed": "9d0e8f7a-6b5c-4d3e-8f1a-2b3c4d5e6f70",
        "advertiseHost": "foldz.example",
        "advertisePort": 8081,
        "registry": { "url": "http://registry:8010", "dnsSdDomain": "example" }
    });
    v["domains"] = json!([{ "name": "a", "path": "/mxl/a" }, { "name": "b", "path": "/mxl/b" }]);
    v["inputs"][0]["domain"] = json!("b");
    v["outputs"][0]["domain"] = json!("a");
    v["outputs"][0]["input"] = json!("main");
    v["outputs"][0]["lfe"] = json!({ "include": { "gainDb": -10 } });
    v["outputs"][0]["gainDb"] = json!(-3.5);
    parse(v).unwrap().validate().unwrap();
}

#[test]
fn unknown_fields_are_rejected_everywhere() {
    let paths: [&[&str]; 5] = [
        &[],
        &["domains", "0"],
        &["inputs", "0"],
        &["outputs", "0"],
        &["nmos"],
    ];
    for path in paths {
        let mut v = base();
        v["nmos"] = json!({});
        let mut at = &mut v;
        for key in path {
            at = match key.parse::<usize>() {
                Ok(i) => &mut at[i],
                Err(_) => &mut at[*key],
            };
        }
        at["bogus"] = json!(1);
        let e = parse(v).unwrap_err();
        assert!(
            e.to_string().contains("unknown field `bogus`"),
            "{path:?}: {e}"
        );
    }
    let mut v = base();
    v["nmos"] = json!({ "registry": { "bogus": 1 } });
    assert!(parse(v).is_err());
    let mut v = base();
    v["outputs"][0]["lfe"] = json!({ "include": { "gainDb": 0, "bogus": 1 } });
    assert!(parse(v).is_err());
}

#[test]
fn bad_layouts_and_presets_fail_to_parse() {
    let mut v = base();
    v["inputs"][0]["layout"][7] = json!("X");
    assert!(
        parse(v)
            .unwrap_err()
            .to_string()
            .contains("unknown channel label")
    );
    let mut v = base();
    v["outputs"][0]["layout"] = json!(["L", "L"]);
    assert!(parse(v).unwrap_err().to_string().contains("more than once"));
    let mut v = base();
    v["outputs"][0]["preset"] = json!("a52-loro");
    assert!(parse(v).unwrap_err().to_string().contains("unknown preset"));
    let mut v = base();
    v["outputs"][0]["lfe"] = json!("keep");
    assert!(parse(v).is_err());
}

#[test]
fn uuids_are_lowercase_canonical() {
    assert!(is_lowercase_uuid(OUT_A));
    for bad in [
        "0F3A6C2E-5B1D-4E8A-9C7F-2D4B6A8E0C11",
        "0f3a6c2e5b1d4e8a9c7f2d4b6a8e0c11",
        "{0f3a6c2e-5b1d-4e8a-9c7f-2d4b6a8e0c11}",
        "urn:uuid:0f3a6c2e-5b1d-4e8a-9c7f-2d4b6a8e0c11",
        "0f3a6c2e-5b1d-4e8a-9c7f-2d4b6a8e0c1g",
        "0f3a6c2e-5b1d-4e8a-9c7f2-d4b6a8e0c11",
        "",
    ] {
        assert!(!is_lowercase_uuid(bad), "{bad}");
    }
    let mut v = base();
    v["outputs"][0]["flows"] = json!([OUT_A.to_uppercase()]);
    assert_invalid(v, "outputs[0].flows[0]", "not a lowercase UUID");
    let mut v = base();
    v["inputs"][0]["flow"] = json!("main");
    assert_invalid(v, "inputs[0].flow", "not a lowercase UUID");
    let mut v = base();
    v["nmos"] = json!({ "seed": "x" });
    assert_invalid(v, "nmos.seed", "not a lowercase UUID");
}

#[test]
fn input_flow_may_be_null_or_absent() {
    let mut v = base();
    v["inputs"][0]["flow"] = Value::Null;
    parse(v).unwrap().validate().unwrap();
    let mut v = base();
    v["inputs"][0].as_object_mut().unwrap().remove("flow");
    parse(v).unwrap().validate().unwrap();
}

#[test]
fn layout_length_matches_channels() {
    let mut v = base();
    v["inputs"][0]["channels"] = json!(16);
    assert_invalid(v, "inputs[0].layout", "8 labels for 16 channels");
}

#[test]
fn domains_are_checked() {
    let mut v = base();
    v["domains"] = json!([]);
    assert_invalid(v, "domains", "at least one");
    let mut v = base();
    v["domains"] = json!([{ "name": "a", "path": "/x" }, { "name": "a", "path": "/y" }]);
    assert_invalid(v, "domains[1]", "used twice");
    let mut v = base();
    v["domains"] = json!([{ "name": "a", "path": "/x" }, { "name": "b", "path": "/x" }]);
    assert_invalid(v, "domains[1]", "path \"/x\" is used twice");
    let mut v = base();
    v["domains"] = json!([{ "name": "a", "path": "mxl/a" }]);
    assert_invalid(v, "domains[0]", "not absolute");
}

#[test]
fn domain_is_required_with_several_and_must_exist() {
    let mut v = base();
    v["domains"] = json!([{ "name": "a", "path": "/mxl/a" }, { "name": "b", "path": "/mxl/b" }]);
    assert_invalid(
        v.clone(),
        "inputs[0].domain",
        "required with more than one domain",
    );
    v["inputs"][0]["domain"] = json!("a");
    assert_invalid(
        v.clone(),
        "outputs[0].domain",
        "required with more than one domain",
    );
    v["outputs"][0]["domain"] = json!("c");
    assert_invalid(v, "outputs[0].domain", "no domain named \"c\"");
}

#[test]
fn input_is_required_with_several_and_must_exist() {
    let mut v = base();
    let second = json!({ "name": "aux", "channels": 2, "layout": ["L", "R"] });
    v["inputs"].as_array_mut().unwrap().push(second);
    assert_invalid(
        v.clone(),
        "outputs[0].input",
        "required with more than one input",
    );
    v["outputs"][0]["input"] = json!("nope");
    assert_invalid(v.clone(), "outputs[0].input", "no input named \"nope\"");
    // The output reads aux, which has no C.
    v["outputs"][0]["input"] = json!("aux");
    assert_invalid(v, "outputs[0]", "reads channel C");
}

#[test]
fn names_are_unique_across_inputs_and_outputs() {
    let mut v = base();
    v["outputs"][0]["name"] = json!("main");
    assert_invalid(v, "outputs[0]", "used twice");
    let mut v = base();
    let dup = v["inputs"][0].clone();
    v["inputs"].as_array_mut().unwrap().push(dup);
    assert_invalid(v, "inputs[1]", "used twice");
}

/// Lowercase UUIDs differing in the last two hex digits.
fn id(n: u8) -> String {
    format!("0f3a6c2e-5b1d-4e8a-9c7f-2d4b6a8e0c{n:02x}")
}

#[test]
fn member_flows_follow_the_replica_rules() {
    let mut v = base();
    v["outputs"][0]["flows"] = json!([]);
    assert_invalid(v, "outputs[0].flows", "1 to 7");
    let mut v = base();
    v["outputs"][0]["flows"] = json!((0..8).map(id).collect::<Vec<_>>());
    v["outputs"][0]["mergedFlow"] = json!(id(0x80));
    assert_invalid(v, "outputs[0].flows", "1 to 7");
    let mut v = base();
    v["outputs"][0]["flows"] = json!([OUT_A, OUT_A]);
    v["outputs"][0]["mergedFlow"] = json!(id(0x80));
    assert_invalid(v, "outputs[0].flows[1]", "used twice");
    let mut v = base();
    v["outputs"][0]["flows"] = json!([IN_A]);
    assert_invalid(v, "outputs[0].flows[0]", "also an input flow");

    // Three replicas: every output names three members and a merged flow,
    // all distinct.
    let mut v = base();
    v["outputs"][0]["flows"] = json!([id(0x30), id(0x31), id(0x32)]);
    v["outputs"][0]["mergedFlow"] = json!(id(0x3f));
    let mut second = v["outputs"][0].clone();
    second["name"] = json!("mono");
    second["layout"] = json!(["C"]);
    second["preset"] = json!("bs775-3_2-to-1_0");
    second["flows"] = json!([id(0x40), id(0x41), id(0x42)]);
    second["mergedFlow"] = json!(id(0x4f));
    v["outputs"].as_array_mut().unwrap().push(second);
    parse(v.clone()).unwrap().validate().unwrap();
    let mut w = v.clone();
    w["outputs"][1]["flows"] = json!([id(0x40), id(0x41)]);
    assert_invalid(
        w,
        "outputs[1].flows",
        "every replica writes one member flow",
    );
    let mut w = v.clone();
    w["outputs"][1]["flows"] = json!([OUT_B, id(0x31), id(0x42)]);
    assert_invalid(w, "outputs[1].flows[1]", "used twice");
}

#[test]
fn merged_flow_follows_the_member_count() {
    let mut v = base();
    v["outputs"][0]["mergedFlow"] = json!(OUT_B);
    assert_invalid(v, "outputs[0].mergedFlow", "more than one member flow only");

    let mut v = base();
    v["outputs"][0]["flows"] = json!([OUT_A, OUT_B]);
    assert_invalid(
        v.clone(),
        "outputs[0].mergedFlow",
        "required with more than one",
    );
    for (merged, reason) in [
        (OUT_A.to_owned(), "used twice"),
        (OUT_B.to_owned(), "used twice"),
        (IN_A.to_owned(), "also an input flow"),
        (OUT_A.to_uppercase(), "not a lowercase UUID"),
    ] {
        let mut w = v.clone();
        w["outputs"][0]["mergedFlow"] = json!(merged);
        assert_invalid(w, "outputs[0].mergedFlow", reason);
    }

    // A merged id is unique across outputs too.
    v["outputs"][0]["mergedFlow"] = json!(id(0x50));
    parse(v.clone()).unwrap().validate().unwrap();
    let mut second = v["outputs"][0].clone();
    second["name"] = json!("mono");
    second["layout"] = json!(["C"]);
    second["preset"] = json!("bs775-3_2-to-1_0");
    second["flows"] = json!([id(0x60), id(0x50)]);
    second["mergedFlow"] = json!(id(0x6f));
    v["outputs"].as_array_mut().unwrap().push(second);
    assert_invalid(v, "outputs[1].flows[1]", "used twice");
}

#[test]
fn two_inputs_may_not_read_one_flow() {
    let mut v = base();
    let mut second = v["inputs"][0].clone();
    second["name"] = json!("copy");
    v["inputs"].as_array_mut().unwrap().push(second);
    v["outputs"][0]["input"] = json!("main");
    assert_invalid(v, "inputs[1].flow", "read by another input");
}

#[test]
fn errors_name_the_item_they_concern() {
    let mut v = base();
    v["outputs"][0]["flows"] = json!(["x"]);
    let e = invalid(v);
    assert_eq!(e.at, "outputs[0].flows[0]");
    assert!(e.reason.starts_with("output \"stereo\": "), "{e}");
    let mut v = base();
    v["inputs"][0]["channels"] = json!(2);
    let e = invalid(v);
    assert_eq!(e.at, "inputs[0].layout");
    assert!(e.reason.starts_with("input \"main\": "), "{e}");
}

#[test]
fn registry_url_and_dns_sd_domain_are_checked() {
    for ok in [
        "http://registry:8010",
        "https://registry.example.com/x-nmos/registration/v1.3",
        "http://10.0.0.1:80",
        "http://[fd00::1]:8010/",
    ] {
        let mut v = base();
        v["nmos"] = json!({ "registry": { "url": ok, "dnsSdDomain": "example.com." } });
        parse(v).unwrap().validate().unwrap();
    }
    for bad in [
        "registry:8010",
        "ftp://registry",
        "http://",
        "http://reg istry",
        "http://registry:0",
        "http://registry:65536",
        "http://registry:80x",
        "http://-registry",
        "http://[fd00::1",
        "http://registry/a b",
        "http://999.0.0.1:8010",
        "http://10.1:8010",
        "http://1.2.3.4.:8010",
        "http://1234",
    ] {
        let mut v = base();
        v["nmos"] = json!({ "registry": { "url": bad } });
        assert_invalid(v, "nmos.registry.url", "not an http or https URL");
    }
    for bad in ["exa mple.com", "-a.com", "a..com", ".", &"a".repeat(64)] {
        let mut v = base();
        v["nmos"] = json!({ "registry": { "dnsSdDomain": bad } });
        assert_invalid(v, "nmos.registry.dnsSdDomain", "not a DNS name");
    }
}

#[test]
fn preset_matrix_and_source_combinations() {
    let mut v = base();
    v["outputs"][0]["matrix"] = json!([[1, 0, 0, 0, 0, 0, 0, 0], [0, 1, 0, 0, 0, 0, 0, 0]]);
    assert_invalid(v.clone(), "outputs[0]", "mutually exclusive");
    let o = v["outputs"][0].as_object_mut().unwrap();
    o.remove("preset");
    o.remove("lfe");
    let folds = parse(v.clone()).unwrap().validate().unwrap();
    assert_eq!(folds[0].consumed, [0, 1]);
    v["outputs"][0].as_object_mut().unwrap().remove("matrix");
    assert_invalid(v.clone(), "outputs[0]", "preset or matrix is required");

    v["outputs"][0]["preset"] = json!("shuffle");
    assert_invalid(
        v.clone(),
        "outputs[0].source",
        "required with preset shuffle",
    );
    v["outputs"][0]["source"] = json!([6, 7]);
    parse(v.clone()).unwrap().validate().unwrap();
    v["outputs"][0]["source"] = json!([6, 8]);
    assert_invalid(v.clone(), "outputs[0]", "input channel 8");
    v["outputs"][0]["preset"] = json!("select");
    assert_invalid(v.clone(), "outputs[0].source", "preset shuffle only");
    v["outputs"][0].as_object_mut().unwrap().remove("source");
    parse(v).unwrap().validate().unwrap();
}

#[test]
fn preset_applicability_is_checked() {
    let mut v = base();
    v["outputs"][0]["layout"] = json!(["L", "R", "C"]);
    assert_invalid(v, "outputs[0]", "bs775-3_2-to-2_0 produces [L, R]");
    let mut v = base();
    v["outputs"][0]["preset"] = json!("ac4-7_1-to-5_1");
    v["outputs"][0]["layout"] = json!(["L", "R", "C", "LFE", "Ls", "Rs"]);
    v["outputs"][0].as_object_mut().unwrap().remove("lfe");
    assert_invalid(v, "outputs[0]", "reads channel Lss");
    let mut v = base();
    v["outputs"][0]["preset"] = json!("bs775-3_2-to-3_0");
    v["outputs"][0]["layout"] = json!(["L", "R", "C"]);
    v["outputs"][0]["lfe"] = json!({ "include": { "gainDb": -10 } });
    assert_invalid(v, "outputs[0]", "1/0 and 2/0 outputs only");
}

#[test]
fn gain_limits_are_checked() {
    let mut v = base();
    v["outputs"][0]["gainDb"] = json!(200);
    assert_invalid(v, "outputs[0]", "exceeds");
    let mut v = base();
    v["outputs"][0]["preset"] = Value::Null;
    v["outputs"][0].as_object_mut().unwrap().remove("lfe");
    v["outputs"][0]["matrix"] = json!([[2e6, 0, 0, 0, 0, 0, 0, 0], [0, 1, 0, 0, 0, 0, 0, 0]]);
    assert_invalid(v, "outputs[0]", "exceeds");
}

#[test]
fn nmos_needs_identity_when_enabled() {
    let mut v = base();
    v["nmos"] = json!({ "enabled": true, "advertiseHost": "h" });
    assert_invalid(v, "nmos", "seed or seedName");
    let mut v = base();
    v["nmos"] = json!({ "enabled": true, "seedName": "ns/foldz" });
    assert_invalid(v, "nmos.advertiseHost", "required");
    let mut v = base();
    v["nmos"] = json!({ "enabled": true, "seedName": "ns/foldz", "advertiseHost": "h" });
    parse(v).unwrap().validate().unwrap();
    let mut v = base();
    v["nmos"] = json!({ "seed": OUT_B, "seedName": "ns/foldz" });
    assert_invalid(v, "nmos", "mutually exclusive");
}

#[test]
fn inputs_and_outputs_are_required() {
    let mut v = base();
    v["outputs"] = json!([]);
    assert_invalid(v, "outputs", "at least one");
    let mut v = base();
    v["inputs"] = json!([]);
    assert_invalid(v, "inputs", "at least one");
}
