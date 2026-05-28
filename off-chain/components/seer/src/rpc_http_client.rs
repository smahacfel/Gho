use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, USER_AGENT};
use reqwest::Url;
use solana_client::nonblocking::rpc_client::RpcClient as AsyncRpcClient;
use solana_client::rpc_client::{RpcClient, RpcClientConfig};
use solana_rpc_client::http_sender::HttpSender;
use solana_sdk::commitment_config::CommitmentConfig;

pub const RPC_HTTP_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36";
pub const RPC_HTTP_AUTH_HEADER_ENV: &str = "GHOST_RPC_AUTH_HEADER";
pub const RPC_HTTP_AUTH_TOKEN_ENV: &str = "GHOST_RPC_AUTH_TOKEN";

const DEFAULT_RPC_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_RPC_AUTH_HEADER: &str = "x-api-key";
const LEGACY_PROVIDER_AUTH_HEADER_ENV: &str = "GHOST_SEER_GRPC_AUTH_HEADER";
const LEGACY_PROVIDER_AUTH_TOKEN_ENV: &str = "GHOST_SEER_GRPC_X_TOKEN";
const NLN_RPC_HOST: &str = "rpc.nln.clr3.org";
const NLN_RPC_HOST_SUFFIX: &str = ".nln.clr3.org";

#[derive(Debug, Clone, PartialEq, Eq)]
struct RpcAuthConfig {
    header_name: String,
    token: String,
}

fn is_nln_rpc_url(url: &str) -> bool {
    let Ok(parsed) = Url::parse(url) else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    host == NLN_RPC_HOST || host.ends_with(NLN_RPC_HOST_SUFFIX)
}

fn non_empty_env_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn rpc_auth_config_for_url_with_lookup<F>(url: &str, lookup: F) -> Option<RpcAuthConfig>
where
    F: Fn(&str) -> Option<String>,
{
    if !is_nln_rpc_url(url) {
        return None;
    }

    let token =
        lookup(RPC_HTTP_AUTH_TOKEN_ENV).or_else(|| lookup(LEGACY_PROVIDER_AUTH_TOKEN_ENV))?;
    let header_name = lookup(RPC_HTTP_AUTH_HEADER_ENV)
        .or_else(|| lookup(LEGACY_PROVIDER_AUTH_HEADER_ENV))
        .unwrap_or_else(|| DEFAULT_RPC_AUTH_HEADER.to_string());

    Some(RpcAuthConfig { header_name, token })
}

fn rpc_auth_config_for_url(url: &str) -> Option<RpcAuthConfig> {
    rpc_auth_config_for_url_with_lookup(url, non_empty_env_value)
}

fn rpc_http_headers(url: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(RPC_HTTP_USER_AGENT));
    if let Some(auth_config) = rpc_auth_config_for_url(url) {
        let header_name = HeaderName::from_bytes(auth_config.header_name.as_bytes())
            .expect("valid RPC auth header name");
        let token =
            HeaderValue::from_str(&auth_config.token).expect("valid RPC auth token header value");
        headers.insert(header_name, token);
    }
    headers
}

fn build_http_client(url: &str, timeout: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .default_headers(rpc_http_headers(url))
        .timeout(timeout)
        .pool_idle_timeout(timeout)
        .build()
        .expect("build rpc http client")
}

pub fn new_async_rpc_client(url: impl Into<String>) -> AsyncRpcClient {
    new_async_rpc_client_with_timeout(url, DEFAULT_RPC_TIMEOUT)
}

pub fn new_async_rpc_client_with_timeout(
    url: impl Into<String>,
    timeout: Duration,
) -> AsyncRpcClient {
    let url = url.into();
    let sender = HttpSender::new_with_client(url.clone(), build_http_client(&url, timeout));
    AsyncRpcClient::new_sender(
        sender,
        RpcClientConfig::with_commitment(CommitmentConfig::default()),
    )
}

pub fn new_blocking_rpc_client(url: impl Into<String>) -> RpcClient {
    new_blocking_rpc_client_with_timeout(url, DEFAULT_RPC_TIMEOUT)
}

pub fn new_blocking_rpc_client_with_timeout(
    url: impl Into<String>,
    timeout: Duration,
) -> RpcClient {
    let url = url.into();
    let sender = HttpSender::new_with_client(url.clone(), build_http_client(&url, timeout));
    RpcClient::new_sender(
        sender,
        RpcClientConfig::with_commitment(CommitmentConfig::default()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpc_http_headers_use_browser_user_agent() {
        let headers = rpc_http_headers("https://api.devnet.solana.com");
        assert_eq!(
            headers
                .get(USER_AGENT)
                .and_then(|value| value.to_str().ok()),
            Some(RPC_HTTP_USER_AGENT)
        );
    }

    #[test]
    fn rpc_auth_config_is_not_added_for_non_nln_urls() {
        let auth =
            rpc_auth_config_for_url_with_lookup("https://api.mainnet-beta.solana.com", |key| {
                match key {
                    RPC_HTTP_AUTH_HEADER_ENV => Some("x-api-key".to_string()),
                    RPC_HTTP_AUTH_TOKEN_ENV => Some("secret-token".to_string()),
                    _ => None,
                }
            });

        assert_eq!(auth, None);
    }

    #[test]
    fn rpc_auth_config_uses_explicit_rpc_env_for_nln_urls() {
        let auth =
            rpc_auth_config_for_url_with_lookup("https://rpc.nln.clr3.org", |key| match key {
                RPC_HTTP_AUTH_HEADER_ENV => Some("x-api-key".to_string()),
                RPC_HTTP_AUTH_TOKEN_ENV => Some("rpc-token".to_string()),
                LEGACY_PROVIDER_AUTH_HEADER_ENV => Some("x-token".to_string()),
                LEGACY_PROVIDER_AUTH_TOKEN_ENV => Some("grpc-token".to_string()),
                _ => None,
            });

        assert_eq!(
            auth,
            Some(RpcAuthConfig {
                header_name: "x-api-key".to_string(),
                token: "rpc-token".to_string(),
            })
        );
    }

    #[test]
    fn rpc_auth_config_falls_back_to_grpc_provider_env_for_nln_urls() {
        let auth =
            rpc_auth_config_for_url_with_lookup("https://rpc.nln.clr3.org/", |key| match key {
                LEGACY_PROVIDER_AUTH_HEADER_ENV => Some("x-api-key".to_string()),
                LEGACY_PROVIDER_AUTH_TOKEN_ENV => Some("grpc-token".to_string()),
                _ => None,
            });

        assert_eq!(
            auth,
            Some(RpcAuthConfig {
                header_name: "x-api-key".to_string(),
                token: "grpc-token".to_string(),
            })
        );
    }
}
