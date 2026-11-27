//! Network policy and the **only** place that builds HTTP clients (P2-T06, SECURITY_MODEL
//! "Network modes", SEC-07).
//!
//! - Local services (the Ollama server) must be a **loopback IP literal** (`127.0.0.0/8` or
//!   `::1`) in every mode. `localhost` is refused because a hosts-file entry or resolver could
//!   point it anywhere; an IP literal can't be redirected by DNS.
//! - Redirects are followed only to loopback targets, so a local server can't bounce a request
//!   to the internet.
//! - Clients never use a system proxy: local traffic must not leave the machine through one.
//! - Internet access (`web.search`, P7) doesn't exist yet. In `local_only` it is always refused.
//!
//! Model downloads in `local_only` mode are done **by the Ollama server** (`/api/pull`, started
//! explicitly by the user). Loki itself only talks to loopback.
//!
//! A test fails if any other crate's `src/` constructs a `reqwest::Client`.

use std::net::IpAddr;
use std::time::Duration;

use reqwest::Url;
use reqwest::redirect::{Attempt, Policy};

use crate::config::NetworkMode;

/// How long to wait for a TCP connection to a local service before reporting it as down.
const LOCAL_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// Redirect hops allowed (all of them must stay on loopback).
const MAX_REDIRECTS: usize = 3;

/// Why a URL or client was refused by policy.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NetworkError {
    /// Not a parseable `http`/`https` URL.
    #[error("invalid URL '{0}'")]
    InvalidUrl(String),
    /// A local service must be on a loopback IP literal.
    #[error("'{0}' is not a loopback address; use 127.0.0.1 or [::1] (not 'localhost')")]
    NotLoopback(String),
    /// URLs with embedded credentials are refused: secrets never live in config.
    #[error("URLs with a username or password are not allowed")]
    CredentialsInUrl,
    /// The network mode forbids this request.
    #[error("blocked by network mode '{mode}': {url}")]
    Blocked {
        /// The mode that refused it.
        mode: &'static str,
        /// The refused URL.
        url: String,
    },
    /// The HTTP client could not be built.
    #[error("could not build HTTP client: {0}")]
    Client(String),
}

/// `true` if the URL's host is a loopback **IP literal**. Host names, including `localhost`, are not.
pub fn is_loopback_ip_url(url: &Url) -> bool {
    url.host_str()
        .map(|host| host.trim_start_matches('[').trim_end_matches(']'))
        .and_then(|host| host.parse::<IpAddr>().ok())
        .is_some_and(|ip| ip.is_loopback())
}

/// Validates the base URL of a local service such as Ollama. Applies in every network mode.
pub fn validate_local_service_url(raw: &str) -> Result<Url, NetworkError> {
    let url = Url::parse(raw).map_err(|_| NetworkError::InvalidUrl(raw.to_owned()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(NetworkError::InvalidUrl(raw.to_owned()));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(NetworkError::CredentialsInUrl);
    }
    if !is_loopback_ip_url(&url) {
        return Err(NetworkError::NotLoopback(
            url.host_str().unwrap_or("").to_owned(),
        ));
    }
    Ok(url)
}

/// Whether `mode` allows an outbound request to `url` that is **not** a local service.
///
/// P2 has no internet features, so this refuses everything non-loopback in `local_only`, and in
/// `tools_allowed` too until P7 adds the guarded internet client (private-address and DNS
/// rebinding checks, SEC-08).
pub fn check_outbound(url: &Url, mode: NetworkMode) -> Result<(), NetworkError> {
    if is_loopback_ip_url(url) {
        return Ok(());
    }
    Err(NetworkError::Blocked {
        mode: match mode {
            NetworkMode::LocalOnly => "local_only",
            NetworkMode::ToolsAllowed => "tools_allowed (internet tools arrive in P7)",
        },
        url: url.to_string(),
    })
}

fn loopback_only_redirects(attempt: Attempt<'_>) -> reqwest::redirect::Action {
    if attempt.previous().len() >= MAX_REDIRECTS {
        attempt.error("too many redirects")
    } else if is_loopback_ip_url(attempt.url()) {
        attempt.follow()
    } else {
        let target = attempt.url().to_string();
        attempt.error(NetworkError::Blocked {
            mode: "loopback-only client",
            url: target,
        })
    }
}

/// Builds HTTP clients that enforce the network policy. Owned by the runtime; other crates
/// receive a ready client and never build their own.
#[derive(Debug, Clone, Copy)]
pub struct HttpClients {
    mode: NetworkMode,
}

impl HttpClients {
    /// A factory for the given network mode.
    pub const fn new(mode: NetworkMode) -> Self {
        Self { mode }
    }

    /// The active network mode.
    pub const fn mode(&self) -> NetworkMode {
        self.mode
    }

    /// A client for a local service at `base_url` (validated as a loopback IP literal), with
    /// loopback-only redirects and no proxy. Returns the client and the parsed URL.
    pub fn local_service(&self, base_url: &str) -> Result<(reqwest::Client, Url), NetworkError> {
        let url = validate_local_service_url(base_url)?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(LOCAL_CONNECT_TIMEOUT)
            .redirect(Policy::custom(loopback_only_redirects))
            .build()
            .map_err(|error| NetworkError::Client(error.to_string()))?;
        Ok((client, url))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn url(raw: &str) -> Url {
        Url::parse(raw).unwrap()
    }

    #[test]
    fn loopback_ip_literals_are_local() {
        for raw in [
            "http://127.0.0.1:11434",
            "http://127.8.9.10/",
            "http://[::1]:11434/api",
        ] {
            assert!(validate_local_service_url(raw).is_ok(), "{raw}");
        }
    }

    #[test]
    fn names_and_non_loopback_addresses_are_refused_for_local_services() {
        for raw in [
            "http://localhost:11434",
            "http://ollama.local:11434",
            "http://192.168.1.20:11434",
            "http://10.0.0.1:11434",
            "http://0.0.0.0:11434",
            "http://example.com",
            "http://127.0.0.1.nip.io:11434",
        ] {
            assert!(
                matches!(
                    validate_local_service_url(raw),
                    Err(NetworkError::NotLoopback(_))
                ),
                "{raw}"
            );
        }
    }

    #[test]
    fn bad_schemes_and_credentials_are_refused() {
        assert!(matches!(
            validate_local_service_url("file:///etc/passwd"),
            Err(NetworkError::InvalidUrl(_))
        ));
        assert!(matches!(
            validate_local_service_url("not a url"),
            Err(NetworkError::InvalidUrl(_))
        ));
        assert_eq!(
            validate_local_service_url("http://user:secret@127.0.0.1:11434"),
            Err(NetworkError::CredentialsInUrl)
        );
    }

    #[test]
    fn local_only_blocks_external_hosts_and_allows_loopback() {
        let mode = NetworkMode::LocalOnly;
        assert!(check_outbound(&url("http://127.0.0.1:8080/x"), mode).is_ok());
        for raw in [
            "https://example.com/",
            "http://93.184.216.34/",
            "http://localhost/",
        ] {
            assert!(matches!(
                check_outbound(&url(raw), mode),
                Err(NetworkError::Blocked { .. })
            ));
        }
    }

    #[test]
    fn tools_allowed_still_blocks_internet_until_p7() {
        assert!(check_outbound(&url("https://example.com/"), NetworkMode::ToolsAllowed).is_err());
    }

    #[tokio::test]
    async fn local_client_reaches_loopback_server() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::path("/api/version"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_string("{\"version\":\"x\"}"),
            )
            .mount(&server)
            .await;
        let (client, base) = HttpClients::new(NetworkMode::LocalOnly)
            .local_service(&server.uri())
            .unwrap();
        let response = client
            .get(base.join("/api/version").unwrap())
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
    }

    /// SEC-07: a redirect from loopback to the internet is not followed.
    #[tokio::test]
    async fn redirect_from_loopback_to_external_is_blocked() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::path("/bounce"))
            .respond_with(
                wiremock::ResponseTemplate::new(302)
                    .insert_header("location", "http://example.com/steal"),
            )
            .mount(&server)
            .await;
        let (client, base) = HttpClients::new(NetworkMode::LocalOnly)
            .local_service(&server.uri())
            .unwrap();
        let error = client
            .get(base.join("/bounce").unwrap())
            .send()
            .await
            .unwrap_err();
        assert!(error.is_redirect(), "{error:?}");
    }

    #[tokio::test]
    async fn redirect_within_loopback_is_followed() {
        let server = wiremock::MockServer::start().await;
        let target = format!("{}/final", server.uri());
        wiremock::Mock::given(wiremock::matchers::path("/hop"))
            .respond_with(
                wiremock::ResponseTemplate::new(302).insert_header("location", target.as_str()),
            )
            .mount(&server)
            .await;
        wiremock::Mock::given(wiremock::matchers::path("/final"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let (client, base) = HttpClients::new(NetworkMode::LocalOnly)
            .local_service(&server.uri())
            .unwrap();
        let response = client.get(base.join("/hop").unwrap()).send().await.unwrap();
        assert_eq!(response.url().path(), "/final");
    }

    /// P2-T06: no crate other than this module builds its own HTTP client.
    #[test]
    fn no_other_crate_builds_a_reqwest_client() {
        fn visit(dir: &Path, offenders: &mut Vec<String>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    visit(&path, offenders);
                } else if path.extension().is_some_and(|e| e == "rs")
                    && !path.ends_with("loki-runtime/src/net.rs")
                {
                    let source = std::fs::read_to_string(&path).unwrap();
                    for needle in ["Client::new(", "Client::builder(", "ClientBuilder::new("] {
                        if source.contains(needle) {
                            offenders.push(format!("{} ({needle})", path.display()));
                        }
                    }
                }
            }
        }
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut offenders = Vec::new();
        for entry in std::fs::read_dir(&crates).unwrap() {
            visit(&entry.unwrap().path().join("src"), &mut offenders);
        }
        assert!(
            offenders.is_empty(),
            "HTTP clients built outside loki-runtime::net: {offenders:?}"
        );
    }
}
