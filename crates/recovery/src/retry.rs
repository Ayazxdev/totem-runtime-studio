//! Simple retry-on-failure logic.

use sentinel_core::error::{SentinelError, SentinelResult};
use std::future::Future;

pub async fn retry_with_backoff<F, Fut, T>(
    mut f: F,
    max_attempts: u32,
) -> SentinelResult<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = SentinelResult<T>>,
{
    let mut attempt = 0;
    loop {
        attempt += 1;
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) if attempt >= max_attempts => {
                return Err(SentinelError::RecoveryFailed {
                    attempts: attempt,
                    reason: format!("{:?}", e),
                });
            }
            Err(e) => {
                tracing::warn!("Attempt {} failed: {:?}, retrying...", attempt, e);
                let backoff_ms = 100 * 2u64.pow(attempt - 1);
                tokio::time::sleep(tokio::time::Duration::from_millis(backoff_ms)).await;
            }
        }
    }
}
