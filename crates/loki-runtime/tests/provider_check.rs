//! Settings' provider check: reachability, installed models (cloud models never listed), the
//! preselected model, and a clear message when Ollama isn't running. A wiremock server on
//! loopback stands in for Ollama.

#![allow(clippy::unwrap_used)]

use loki_runtime::{ProviderKind, Runtime, RuntimeOptions, config};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Opens a mock-provider runtime whose config points Ollama at `base_url`, with `saved` as the
/// saved model.
fn runtime(dir: &tempfile::TempDir, base_url: &str, saved: Option<&str>) -> Runtime {
    let first = Runtime::open(RuntimeOptions {
        data_dir: Some(dir.path().into()),
        provider: Some(ProviderKind::Mock),
        ..RuntimeOptions::default()
    })
    .unwrap();
    let path = first.paths().config_file();
    drop(first);
    let mut cfg = config::load_or_init(&path).unwrap().config;
    cfg.ollama.base_url = base_url.to_owned();
    cfg.model_id = saved.map(str::to_owned);
    config::save(&path, &cfg).unwrap();
    Runtime::open(RuntimeOptions {
        data_dir: Some(dir.path().into()),
        provider: Some(ProviderKind::Mock),
        ..RuntimeOptions::default()
    })
    .unwrap()
}

async fn ollama_with(models: &[(&str, u64)]) -> MockServer {
    let server = MockServer::start().await;
    let list: Vec<_> = models
        .iter()
        .map(|(name, size)| serde_json::json!({ "name": name, "size": size, "digest": "sha256:x" }))
        .collect();
    Mock::given(method("GET"))
        .and(path("/api/tags"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "models": list })),
        )
        .mount(&server)
        .await;
    server
}

#[tokio::test(flavor = "multi_thread")]
async fn lists_installed_local_models_hides_cloud_ones_and_suggests_a_reviewed_one() {
    let server = ollama_with(&[
        ("llama-custom:7b", 4_000_000_000),
        ("gpt-oss:120b-cloud", 0),
        ("bonsai-2:4b", 1_600_000_000),
        ("qwen3:4b", 2_500_000_000),
    ])
    .await;
    let dir = tempfile::tempdir().unwrap();
    let rt = runtime(&dir, &server.uri(), None);
    let check = rt.check_provider().await;
    assert!(check.reachable, "{check:?}");
    assert_eq!(check.detail, None);
    assert_eq!(check.hidden_cloud_models, 1);
    let ids: Vec<_> = check.models.iter().map(|m| m.id.as_str()).collect();
    assert!(!ids.iter().any(|id| id.ends_with("-cloud")), "{ids:?}");
    assert_eq!(
        ids.last(),
        Some(&"llama-custom:7b"),
        "unreviewed models come last"
    );
    let custom = check
        .models
        .iter()
        .find(|m| m.id == "llama-custom:7b")
        .unwrap();
    assert!(!custom.reviewed && custom.license.is_none());
    let bonsai = check.models.iter().find(|m| m.id == "bonsai-2:4b").unwrap();
    assert!(bonsai.reviewed && bonsai.license.is_some());
    assert_eq!(bonsai.size_bytes, 1_600_000_000);
    let suggested = check.suggested.unwrap();
    assert!(
        ["bonsai-2:4b", "qwen3:4b"].contains(&suggested.as_str()),
        "a reviewed model is suggested, got {suggested}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_saved_model_is_kept_when_installed_and_replaced_when_not() {
    let server = ollama_with(&[("bonsai-2:4b", 1), ("llama-custom:7b", 1)]).await;
    let dir = tempfile::tempdir().unwrap();
    let rt = runtime(&dir, &server.uri(), Some("llama-custom:7b"));
    assert_eq!(
        rt.check_provider().await.suggested.as_deref(),
        Some("llama-custom:7b")
    );
    let dir = tempfile::tempdir().unwrap();
    let rt = runtime(&dir, &server.uri(), Some("qwen3:8b"));
    assert_eq!(
        rt.check_provider().await.suggested.as_deref(),
        Some("bonsai-2:4b"),
        "the saved model isn't installed, so the reviewed one is suggested"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_empty_ollama_says_how_to_get_a_model() {
    let server = ollama_with(&[]).await;
    let dir = tempfile::tempdir().unwrap();
    let check = runtime(&dir, &server.uri(), None).check_provider().await;
    assert!(check.reachable);
    assert!(check.models.is_empty() && check.suggested.is_none());
    assert!(check.detail.unwrap().contains("loki models pull"));
}

#[tokio::test(flavor = "multi_thread")]
async fn ollama_not_running_is_reported_with_how_to_start_it() {
    // A loopback port nothing listens on.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let dir = tempfile::tempdir().unwrap();
    let check = runtime(&dir, &format!("http://127.0.0.1:{port}"), None)
        .check_provider()
        .await;
    assert!(!check.reachable);
    assert!(check.models.is_empty() && check.suggested.is_none());
    assert!(check.detail.unwrap().contains("ollama-serve.sh"));
}
