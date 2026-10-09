//! FIX-4 finite synthetic host mediation. NOT a provider gateway or A9 path.
//! The runtime receives no secret. Both SDK default forwarders are overridden.
use github_copilot_sdk::copilot_request_handler::{
    CopilotHttpRequest, CopilotHttpResponse, CopilotRequestContext, CopilotRequestError,
    CopilotRequestHandler, CopilotWebSocketHandler, CopilotWebSocketResponse,
};
use std::{
    future::Future,
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream},
    pin::Pin,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

/// SDK 1.0.17 start discards setProvider.success. Require a positive ACK before
/// metadata admission. This is one explicit registration validation, not a retry
/// loop or authorization to forward any provider traffic.
pub async fn require_registered(client: &github_copilot_sdk::Client) -> Result<(), &'static str> {
    let ack = narys_lr10a_poc::bounded(client.rpc().llm_inference().set_provider()).await?;
    if ack.success {
        Ok(())
    } else {
        Err("handler_registration_declined")
    }
}

pub struct FixtureGateway {
    endpoint: Option<SocketAddr>,
    synthetic_secret: Option<String>,
    calls: AtomicUsize,
}
impl FixtureGateway {
    /// Only loopback fixture endpoints; no URL/DNS/proxy/credential-store input.
    pub fn new(port: Option<u16>, synthetic_secret: Option<String>) -> Self {
        Self {
            endpoint: port.map(|p| SocketAddrV4::new(Ipv4Addr::LOCALHOST, p).into()),
            synthetic_secret,
            calls: AtomicUsize::new(0),
        }
    }
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
    fn exchange(&self, request: &CopilotHttpRequest) -> Result<(), CopilotRequestError> {
        let fail = || CopilotRequestError::message("fixture_gateway_blocked");
        // A finite operation, not a domain allowlist or arbitrary HTTP tunnel.
        if request.method != "GET"
            || request.url != "https://fixture.invalid/metadata"
            || !request.body.is_empty()
            || !request.headers.is_empty()
            || request.cancel.is_cancelled()
        {
            return Err(fail());
        }
        let endpoint = self.endpoint.ok_or_else(fail)?;
        let secret = self.synthetic_secret.as_deref().ok_or_else(fail)?;
        // Public synthetic marker required: no real token accepted by this POC.
        if !secret.starts_with("FIX4_SYNTHETIC_")
            || secret.len() > 96
            || !secret
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || self
                .calls
                .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
        {
            return Err(fail());
        }
        // No inherited proxy, DNS, CONNECT, redirect or runtime-chosen address.
        // HTTP ONLY on owned host loopback; remote TLS is deliberately unimplemented.
        let budget = Duration::from_millis(250);
        let end = Instant::now() + budget;
        let mut connection = TcpStream::connect_timeout(&endpoint, budget).map_err(|_| fail())?;
        connection
            .set_write_timeout(Some(budget))
            .map_err(|_| fail())?;
        connection
            .write_all(format!("GET /metadata HTTP/1.1\r\nHost: fixture.invalid\r\nAuthorization: Bearer {secret}\r\nConnection: close\r\n\r\n").as_bytes())
            .map_err(|_| fail())?;
        let mut response = Vec::new();
        let mut buf = [0u8; 256];
        loop {
            let remaining = end
                .checked_duration_since(Instant::now())
                .ok_or_else(fail)?;
            connection
                .set_read_timeout(Some(remaining))
                .map_err(|_| fail())?;
            if request.cancel.is_cancelled() {
                return Err(fail());
            }
            let n = connection.read(&mut buf).map_err(|_| fail())?;
            if n == 0 {
                break;
            }
            response.extend_from_slice(&buf[..n]);
            if response.len() > 1024 {
                return Err(fail());
            }
        }
        // Provider bytes/headers NEVER passed through. Only this finite fixture DTO.
        if response
            != b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"value\":5}"
        {
            return Err(fail());
        }
        Ok(())
    }
}
impl CopilotRequestHandler for FixtureGateway {
    fn send_request<'a, 'b, 'async_trait>(
        &'a self,
        request: CopilotHttpRequest,
        _: &'b CopilotRequestContext,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<CopilotHttpResponse, CopilotRequestError>>
                + Send
                + 'async_trait,
        >,
    >
    where
        'a: 'async_trait,
        'b: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            self.exchange(&request)?;
            Ok(CopilotHttpResponse::new(
                200,
                None,
                Default::default(),
                Box::pin(futures_util::stream::once(async {
                    Ok(bytes::Bytes::from_static(b"{\"value\":5}"))
                })),
            ))
        })
    }
    fn open_websocket<'a, 'b, 'async_trait>(
        &'a self,
        _: &'b CopilotRequestContext,
        _: CopilotWebSocketResponse,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<Box<dyn CopilotWebSocketHandler>, CopilotRequestError>>
                + Send
                + 'async_trait,
        >,
    >
    where
        'a: 'async_trait,
        'b: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async { Err(CopilotRequestError::message("fixture_websocket_blocked")) })
    }
}
