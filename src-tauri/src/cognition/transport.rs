use reqwest::{
    header::{HeaderMap, RETRY_AFTER},
    Error,
};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, SystemTime},
};

use super::types::{ProviderError, ProviderRequest, ProviderTimeouts};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TimeoutPhase {
    Connect,
    Overall,
    StreamIdle,
    Http408,
    Http504,
}
impl TimeoutPhase {
    fn label(self) -> &'static str {
        match self {
            Self::Connect => "connect_timeout",
            Self::Overall => "request_overall_timeout",
            Self::StreamIdle => "stream_idle_timeout",
            Self::Http408 => "http_408",
            Self::Http504 => "http_504",
        }
    }
}
#[derive(Clone, Debug)]
pub(super) struct TimeoutDiagnostic {
    pub provider: &'static str,
    pub model: String,
    pub phase: TimeoutPhase,
    pub attempt: u32,
    pub configured_ms: u64,
    pub elapsed_ms: u64,
}
impl TimeoutDiagnostic {
    pub fn safe_line(&self) -> String {
        let model: String = self
            .model
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '/' | '.' | '_' | '-'))
            .take(128)
            .collect();
        format!("[Provider][diag] provider={} model={} phase={} attempt={} configured_ms={} elapsed_ms={} status={}",
            self.provider, model, self.phase.label(), self.attempt, self.configured_ms, self.elapsed_ms,
            match self.phase { TimeoutPhase::Http408 => "408", TimeoutPhase::Http504 => "504", _ => "none" })
    }
}
#[cfg(test)]
pub(super) static TIMEOUT_DIAGNOSTICS: std::sync::Mutex<Vec<TimeoutDiagnostic>> =
    std::sync::Mutex::new(Vec::new());

pub(super) fn timeout_error(
    provider: &'static str,
    request: &ProviderRequest,
    phase: TimeoutPhase,
    timeouts: ProviderTimeouts,
    connect_ms: u64,
    started: std::time::Instant,
) -> ProviderError {
    let diagnostic = TimeoutDiagnostic {
        provider,
        model: request.target.invocation.model.clone(),
        phase,
        attempt: request.attempt,
        configured_ms: match phase {
            TimeoutPhase::Connect => connect_ms,
            TimeoutPhase::StreamIdle => timeouts.stream_idle_timeout_ms as u64,
            _ => timeouts.request_timeout_ms as u64,
        },
        elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
    };
    #[cfg(debug_assertions)]
    eprintln!("{}", diagnostic.safe_line());
    #[cfg(test)]
    {
        let mut events = TIMEOUT_DIAGNOSTICS
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if events.len() == 1024 {
            events.remove(0);
        }
        events.push(diagnostic);
    }
    #[cfg(not(any(test, debug_assertions)))]
    let _ = diagnostic;
    ProviderError::Timeout
}

pub(super) fn diagnosed_network_error(
    error: &Error,
    provider: &'static str,
    request: &ProviderRequest,
    timeouts: ProviderTimeouts,
    connect_ms: u64,
    started: std::time::Instant,
) -> ProviderError {
    if error.is_timeout() {
        timeout_error(
            provider,
            request,
            if error.is_connect() {
                TimeoutPhase::Connect
            } else {
                TimeoutPhase::Overall
            },
            timeouts,
            connect_ms,
            started,
        )
    } else {
        network_error(error)
    }
}

pub fn retry_after_ms(headers: &HeaderMap) -> Option<u64> {
    const MAX_RETRY_AFTER_MS: u64 = 7 * 24 * 60 * 60 * 1000;
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?;
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(seconds.saturating_mul(1000).min(MAX_RETRY_AFTER_MS));
    }
    httpdate::parse_http_date(value)
        .ok()
        .and_then(|date| date.duration_since(SystemTime::now()).ok())
        .map(|duration| duration.as_millis().min(MAX_RETRY_AFTER_MS as u128) as u64)
}

/// Strict factual counterpart to the existing operational Retry-After parser.
/// No clamped value is presented as a provider-reported fact (RFC 9110 §10.2.3).
pub(super) fn factual_retry_after_ms(headers: &HeaderMap) -> Option<u64> {
    factual_retry_after_at(headers, SystemTime::now())
}
pub(super) fn factual_retry_after_at(headers: &HeaderMap, now: SystemTime) -> Option<u64> {
    const MAX_HINT_MS: u64 = 7 * 24 * 60 * 60 * 1000;
    let mut values = headers.get_all(RETRY_AFTER).iter();
    let value = values.next()?.to_str().ok()?;
    if values.next().is_some() || value.is_empty() || value.len() > 128 {
        return None;
    }
    if value.bytes().all(|b| b.is_ascii_digit()) {
        return value.parse::<u64>().ok()?.checked_mul(1000)
            .filter(|n| *n <= MAX_HINT_MS);
    }
    let duration = httpdate::parse_http_date(value).ok()?.duration_since(now).ok()?;
    u64::try_from(duration.as_millis()).ok().filter(|n| *n <= MAX_HINT_MS)
}

pub fn network_error(error: &Error) -> ProviderError {
    if error.is_timeout() {
        ProviderError::Timeout
    } else {
        ProviderError::Unavailable {
            retry_after_ms: None,
        }
    }
}

pub async fn cancellation(cancelled: &AtomicBool) {
    while !cancelled.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::Duration,
    };

    pub(crate) fn server(
        status: &str,
        body: &str,
        split: bool,
        delay: Duration,
    ) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let status = status.to_owned();
        let body = body.to_owned();
        let handle = thread::spawn(move || {
            let (mut connection, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            let _ = connection.set_read_timeout(Some(Duration::from_secs(2)));
            loop {
                let Ok(read) = connection.read(&mut buffer) else {
                    break;
                };
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            if split {
                for chunk in response.as_bytes().chunks(5) {
                    if connection.write_all(chunk).is_err() {
                        break;
                    }
                    if !delay.is_zero() {
                        thread::sleep(delay);
                    }
                }
            } else {
                let _ = connection.write_all(response.as_bytes());
            }
        });
        (url, handle)
    }
}
