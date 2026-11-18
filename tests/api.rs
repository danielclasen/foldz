//! The `/api/v1` routes behind the samplez-service listeners, the committed
//! OpenAPI document and the replicated state of one replica.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use foldz::api::{self, Downmix};
use foldz::config::Config;
use foldz::state::Standalone;
use samplez_service::config::{AuthConfig, AuthMode, SecretFile};
use samplez_service::openapi::check_committed;
use samplez_service::{Ops, Owner, Position, ReplicatedState, Role, Service, ServiceConfig};
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "6b1f0c3e9a2d4f8b7c5e1a3d9f2b4c6e8a0d2f4b6c8e0a2d4f6b8c0e2a4d6f8b";

fn config() -> Config {
    serde_json::from_value(json!({
        "domains": [{ "name": "a", "path": "/mxl/a" }],
        "inputs": [
            { "name": "main", "channels": 8,
              "layout": ["L", "R", "C", "LFE", "Ls", "Rs", "-", "-"],
              "flow": "7a1e4c9b-3d2f-4b6a-8e0c-5f7d9b1a3c55" },
            { "name": "spare", "channels": 2, "layout": ["L", "R"] }
        ],
        "outputs": [
            { "name": "stereo", "input": "main", "flows": ["0f3a6c2e-5b1d-4e8a-9c7f-2d4b6a8e0c11"],
              "layout": ["L", "R"], "preset": "a52-loro-clev0.707-slev0.707",
              "lfe": { "include": { "gainDb": -10 } }, "gainDb": -3, "group": "prog" },
            { "name": "swap", "input": "spare", "flows": ["0f3a6c2e-5b1d-4e8a-9c7f-2d4b6a8e0c12"],
              "layout": ["L", "R"], "preset": "shuffle", "source": [1, 0], "label": "Swapped" }
        ]
    }))
    .unwrap()
}

/// A token-mode service with no engine running, as after a stop.
fn service() -> Service {
    // One file per call: tests run in parallel and a file being rewritten
    // reads empty.
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let token = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("foldz-api-token-{n}"));
    std::fs::write(&token, TOKEN).unwrap();
    let config = config();
    let folds = config.validate().unwrap();
    let service_config = ServiceConfig {
        auth: AuthConfig {
            mode: AuthMode::Token,
            token: Some(SecretFile { file: token }),
            ..AuthConfig::default()
        },
        ..ServiceConfig::default()
    };
    let state = Arc::new(Standalone::new("test".into()));
    let service = Service::new(service_config, Ops::default(), state).unwrap();
    let downmix = Downmix::new(&config, &folds);
    let router = api::router(
        Arc::new(Mutex::new(None)),
        config,
        downmix,
        service.status(),
        None,
    );
    service.api(router)
}

async fn get(service: &Service, path: &str, bearer: Option<&str>) -> (StatusCode, Value) {
    let mut req = Request::get(path).header(header::HOST, "localhost:8080");
    if let Some(token) = bearer {
        req = req.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let res = service
        .routers()
        .api
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let body = to_bytes(res.into_body(), 1 << 20).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

#[tokio::test]
async fn downmix_is_served_behind_the_auth_mode() {
    let service = service();
    assert_eq!(
        get(&service, "/api/v1/downmix", None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        get(&service, "/api/v1/downmix", Some("junk")).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, body) = get(&service, "/api/v1/downmix", Some(TOKEN)).await;
    assert_eq!(status, StatusCode::OK);

    assert_eq!(
        body["inputs"],
        json!([
            { "name": "main", "domain": "a", "flow": "7a1e4c9b-3d2f-4b6a-8e0c-5f7d9b1a3c55",
              "layout": ["L", "R", "C", "LFE", "Ls", "Rs", "-", "-"] },
            { "name": "spare", "domain": "a", "flow": null, "layout": ["L", "R"] }
        ])
    );
    let stereo = &body["outputs"][0];
    assert_eq!(stereo["preset"], "a52-loro-clev0.707-slev0.707");
    assert_eq!(
        stereo["citation"],
        "ATSC A/52:2018, clause 7.8.2 (Lo/Ro), clev from Table 5.9, slev from Table 5.10"
    );
    assert_eq!(stereo["grouphint"], "prog:Audio");
    assert_eq!(stereo["label"], "stereo");
    assert_eq!(stereo["lfeGainDb"], -10.0);
    assert_eq!(stereo["gainDb"], -3.0);
    // The matrix served is the one the engine applies, gainDb included.
    let folds = config().validate().unwrap();
    let m = &folds[0].matrix;
    for j in 0..2 {
        for i in 0..8 {
            assert_eq!(
                stereo["matrix"][j][i].as_f64().unwrap() as f32,
                m.gain(j, i),
                "{j} {i}"
            );
        }
    }
    let swap = &body["outputs"][1];
    assert_eq!(swap["preset"], "shuffle");
    assert_eq!(swap["citation"], Value::Null);
    assert_eq!(swap["lfeGainDb"], Value::Null);
    assert_eq!(swap["label"], "Swapped");
    assert_eq!(swap["matrix"], json!([[0.0, 1.0], [1.0, 0.0]]));
}

#[tokio::test]
async fn status_without_an_engine_is_unavailable() {
    let service = service();
    assert_eq!(
        get(&service, "/api/v1/status", Some(TOKEN)).await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        get(&service, "/api/v1/nothing", Some(TOKEN)).await.0,
        StatusCode::NOT_FOUND
    );
}

fn committed() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("openapi.json")
}

/// `SAMPLEZ_UPDATE_COMMITTED=1 cargo test --test api committed` rewrites the
/// committed file.
#[test]
fn committed_openapi_document_matches() {
    let doc = serde_json::to_value(api::openapi(AuthMode::Token)).unwrap();
    assert_eq!(doc["openapi"], "3.1.0");
    assert_eq!(doc["info"]["title"], "foldz");
    for path in ["/api/v1/downmix", "/api/v1/status", "/api/v1/login"] {
        assert!(doc["paths"][path].is_object(), "{path}");
    }
    check_committed(&committed(), &doc).unwrap();
}

#[test]
fn openapi_security_follows_the_mode() {
    let none = serde_json::to_value(api::openapi(AuthMode::None)).unwrap();
    assert!(none.get("security").is_none());
    assert!(none["paths"].get("/api/v1/login").is_none());
    assert!(none["paths"]["/api/v1/status"]["get"].is_object());
}

fn owner() -> Owner {
    Owner {
        principal: "anonymous".into(),
        session: "s".into(),
        role: Role::Operator,
        connection: "c".into(),
    }
}

#[tokio::test]
async fn the_standalone_state_takes_no_command() {
    let state = Standalone::new("r0".into());
    let replica = state.replica();
    assert_eq!(
        (replica.id.as_str(), replica.role.as_str()),
        ("r0", "standalone")
    );
    assert!(replica.in_sync && replica.ready && replica.commitable);
    assert_eq!(state.revision(), 0);
    assert!(state.submit(&owner(), "1", "{}".into()).await.is_err());
    assert_eq!(state.command_resolution(&owner(), "1"), None);
    let here = Position {
        epoch: state.epoch(),
        revision: 0,
    };
    assert_eq!(state.deltas_since(here).await.map(|d| d.len()), Some(0));
    let elsewhere = Position {
        epoch: state.epoch().wrapping_add(1),
        revision: 0,
    };
    assert!(state.deltas_since(elsewhere).await.is_none());
    let snapshot = state.snapshot().await;
    assert_eq!((snapshot.epoch, snapshot.revision), (state.epoch(), 0));
    assert_ne!(
        Standalone::new("r0".into()).epoch(),
        state.epoch(),
        "an epoch is drawn per start"
    );
}
