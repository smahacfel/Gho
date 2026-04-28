use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use solana_client::nonblocking::rpc_client::RpcClient as AsyncRpcClient;
use solana_client::rpc_client::{RpcClient, RpcClientConfig};
use solana_rpc_client::http_sender::HttpSender;
use solana_sdk::commitment_config::CommitmentConfig;

pub const RPC_HTTP_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36";

const DEFAULT_RPC_TIMEOUT: Duration = Duration::from_secs(30);

fn rpc_http_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(RPC_HTTP_USER_AGENT));
    headers
}

fn build_http_client(timeout: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .default_headers(rpc_http_headers())
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
    let sender = HttpSender::new_with_client(url, build_http_client(timeout));
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
    let sender = HttpSender::new_with_client(url, build_http_client(timeout));
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
        let headers = rpc_http_headers();
        assert_eq!(
            headers
                .get(USER_AGENT)
                .and_then(|value| value.to_str().ok()),
            Some(RPC_HTTP_USER_AGENT)
        );
    }
}
