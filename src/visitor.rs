use crate::config::Config;
use dashmap::DashMap;
use governor::{
    clock::DefaultClock,
    middleware::NoOpMiddleware,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter,
};
use std::{
    net::IpAddr,
    num::NonZeroU32,
    sync::Arc,
    time::{Duration, Instant},
};

type Limiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock, NoOpMiddleware>;

/// Per-IP state: rate limiters and last-seen timestamp.
pub struct Visitor {
    /// General request rate limiter (token bucket).
    pub request_limiter: Arc<Limiter>,
    /// Tracks active subscription count (simple atomic counter).
    pub subscription_count: std::sync::atomic::AtomicU32,
    pub last_seen: Instant,
}

impl Visitor {
    fn new(config: &Config) -> Self {
        let burst = NonZeroU32::new(config.request_limit_burst).unwrap_or(NonZeroU32::MIN);
        let replenish = Duration::from_secs(config.request_limit_replenish_secs);
        // Quota: `burst` tokens, refilled at 1 token per `replenish / burst`.
        let quota = Quota::with_period(replenish / config.request_limit_burst)
            .unwrap_or(Quota::per_second(burst))
            .allow_burst(burst);
        Visitor {
            request_limiter: Arc::new(RateLimiter::direct(quota)),
            subscription_count: std::sync::atomic::AtomicU32::new(0),
            last_seen: Instant::now(),
        }
    }

    /// Returns true if the request should be allowed through.
    pub fn request_allowed(&self) -> bool {
        self.request_limiter.check().is_ok()
    }

    pub fn subscription_count(&self) -> u32 {
        self.subscription_count
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn increment_subscriptions(&self) {
        self.subscription_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn decrement_subscriptions(&self) {
        self.subscription_count
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// A visitor is stale when it has had no activity for 30 minutes and
    /// holds no active subscriptions.
    pub fn is_stale(&self) -> bool {
        self.subscription_count() == 0
            && self.last_seen.elapsed() > Duration::from_secs(30 * 60)
    }
}

/// Shared map of per-IP visitors. Uses DashMap for lock-free concurrent reads.
pub struct VisitorMap {
    inner: DashMap<IpAddr, Arc<Visitor>>,
    config: Arc<Config>,
}

impl VisitorMap {
    pub fn new(config: Arc<Config>) -> Self {
        VisitorMap {
            inner: DashMap::new(),
            config,
        }
    }

    /// Return the visitor for `ip`, creating one if it does not exist.
    pub fn get_or_create(&self, ip: IpAddr) -> Arc<Visitor> {
        if let Some(v) = self.inner.get(&ip) {
            return Arc::clone(&v);
        }
        let visitor = Arc::new(Visitor::new(&self.config));
        self.inner
            .entry(ip)
            .or_insert_with(|| Arc::clone(&visitor));
        Arc::clone(&self.inner.get(&ip).unwrap())
    }

    /// Remove visitors that have been idle and hold no subscriptions.
    pub fn prune_stale(&self) -> usize {
        let stale: Vec<IpAddr> = self
            .inner
            .iter()
            .filter(|e| e.value().is_stale())
            .map(|e| *e.key())
            .collect();
        let n = stale.len();
        for ip in stale {
            self.inner.remove(&ip);
        }
        n
    }

    pub fn visitor_count(&self) -> usize {
        self.inner.len()
    }
}
