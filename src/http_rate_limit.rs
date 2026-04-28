//! HTTP endpoint rate limiting utilities.
//!
//! Provides IP-based rate limiting for HTTP endpoints to prevent abuse.

use governor::{
    clock::DefaultClock,
    state::{direct::NotKeyed, InMemoryState},
    Quota, RateLimiter,
};
use std::collections::HashMap;
use std::net::IpAddr;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// IP-based rate limiter for HTTP endpoints.
pub struct IpRateLimiter {
    /// Rate limiters per IP address
    limiters:
        Arc<RwLock<HashMap<IpAddr, (RateLimiter<NotKeyed, InMemoryState, DefaultClock>, Instant)>>>,
    /// Quota configuration (requests per minute)
    quota: NonZeroU32,
    /// Cleanup interval for old entries
    cleanup_interval: Duration,
}

impl IpRateLimiter {
    /// Create a new IP-based rate limiter.
    ///
    /// # Arguments
    /// * `requests_per_minute` - Maximum requests allowed per minute per IP
    pub fn new(requests_per_minute: u32) -> Self {
        Self {
            limiters: Arc::new(RwLock::new(HashMap::new())),
            quota: NonZeroU32::new(requests_per_minute)
                .unwrap_or_else(|| NonZeroU32::new(60).unwrap()),
            cleanup_interval: Duration::from_secs(300), // 5 minutes
        }
    }

    /// Check if a request from the given IP is allowed.
    ///
    /// # Arguments
    /// * `ip` - The IP address making the request
    ///
    /// # Returns
    /// * `Ok(())` if request is allowed
    /// * `Err(String)` with retry-after duration if rate limited
    pub async fn check_rate_limit(&self, ip: IpAddr) -> Result<(), String> {
        let mut limiters = self.limiters.write().await;

        // Get or create limiter for this IP
        let (limiter, last_access) = limiters.entry(ip).or_insert_with(|| {
            let quota = Quota::per_minute(self.quota);
            let limiter = RateLimiter::direct(quota);
            (limiter, Instant::now())
        });

        // Update last access time
        *last_access = Instant::now();

        // Check rate limit
        match limiter.check() {
            Ok(_) => Ok(()),
            Err(not_until) => {
                use governor::clock::Clock;
                let now = DefaultClock::default().now();
                let wait_duration = not_until.wait_time_from(now);
                Err(format!("{}", wait_duration.as_secs()))
            }
        }
    }

    /// Cleanup old rate limiter entries (call periodically).
    pub async fn cleanup_old_entries(&self) {
        let mut limiters = self.limiters.write().await;
        let cutoff = Instant::now() - self.cleanup_interval;

        limiters.retain(|_, (_, last_access)| *last_access > cutoff);

        if !limiters.is_empty() {
            tracing::debug!("Rate limiter cleanup: {} active IPs", limiters.len());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[tokio::test]
    async fn test_rate_limiter_allows_requests() {
        let limiter = IpRateLimiter::new(10);
        let ip = IpAddr::from_str("127.0.0.1").unwrap();

        // First few requests should be allowed
        for _ in 0..5 {
            assert!(limiter.check_rate_limit(ip).await.is_ok());
        }
    }

    #[tokio::test]
    async fn test_rate_limiter_blocks_excessive_requests() {
        let limiter = IpRateLimiter::new(5);
        let ip = IpAddr::from_str("127.0.0.1").unwrap();

        // Use up the quota
        for _ in 0..5 {
            let _ = limiter.check_rate_limit(ip).await;
        }

        // Next request should be blocked
        let result = limiter.check_rate_limit(ip).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_rate_limiter_per_ip_isolation() {
        let limiter = IpRateLimiter::new(5);
        let ip1 = IpAddr::from_str("127.0.0.1").unwrap();
        let ip2 = IpAddr::from_str("192.168.1.1").unwrap();

        // Use up quota for ip1
        for _ in 0..5 {
            let _ = limiter.check_rate_limit(ip1).await;
        }

        // ip1 should be blocked
        assert!(limiter.check_rate_limit(ip1).await.is_err());

        // ip2 should still be allowed
        assert!(limiter.check_rate_limit(ip2).await.is_ok());
    }

    #[tokio::test]
    async fn test_rate_limiter_cleanup() {
        let limiter = IpRateLimiter::new(60);
        let ip = IpAddr::from_str("127.0.0.1").unwrap();

        // Make a request to create an entry
        let _ = limiter.check_rate_limit(ip).await;

        // Check that entry exists
        {
            let limiters = limiter.limiters.read().await;
            assert_eq!(limiters.len(), 1);
        }

        // Cleanup shouldn't remove recent entries
        limiter.cleanup_old_entries().await;

        {
            let limiters = limiter.limiters.read().await;
            assert_eq!(limiters.len(), 1);
        }
    }
}
