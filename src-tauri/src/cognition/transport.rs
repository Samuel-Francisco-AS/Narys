use reqwest::{
    header::{HeaderMap, RETRY_AFTER},
    Error,
};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, SystemTime},
};

use super::types::ProviderError;

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
