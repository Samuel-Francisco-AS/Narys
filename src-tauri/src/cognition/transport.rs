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
