//! Metrics for tracking deprecated API usage.
//!
//! Provides Prometheus metrics for monitoring deprecated endpoint access.

use prometheus::{HistogramVec, IntCounterVec, IntGaugeVec, Opts, Registry};

/// Metrics collector for deprecated API usage.
#[derive(Clone)]
pub struct DeprecationMetrics {
    /// Registry for all metrics
    registry: Registry,

    /// Counter for deprecated endpoint requests
    pub requests_total: IntCounterVec,

    /// Counter for redirects performed
    pub redirects_total: IntCounterVec,

    /// Counter for blocked requests (past sunset)
    pub blocked_total: IntCounterVec,

    /// Gauge for days until sunset for each endpoint
    pub days_until_sunset: IntGaugeVec,

    /// Histogram for request latency by deprecated endpoint
    pub request_duration_seconds: HistogramVec,
}

impl DeprecationMetrics {
    /// Create a new metrics collector with the given prefix.
    pub fn new(prefix: &str) -> Self {
        let registry = Registry::new();

        let requests_total = IntCounterVec::new(
            Opts::new(
                format!("{}_requests_total", prefix),
                "Total number of requests to deprecated endpoints",
            ),
            &["endpoint_id", "path", "method", "status"],
        )
        .expect("Failed to create requests_total metric");

        let redirects_total = IntCounterVec::new(
            Opts::new(
                format!("{}_redirects_total", prefix),
                "Total number of redirects from deprecated endpoints",
            ),
            &["endpoint_id", "from_path", "to_path"],
        )
        .expect("Failed to create redirects_total metric");

        let blocked_total = IntCounterVec::new(
            Opts::new(
                format!("{}_blocked_total", prefix),
                "Total number of blocked requests to removed endpoints",
            ),
            &["endpoint_id", "path", "reason"],
        )
        .expect("Failed to create blocked_total metric");

        let days_until_sunset = IntGaugeVec::new(
            Opts::new(
                format!("{}_days_until_sunset", prefix),
                "Days until endpoint sunset (negative if past)",
            ),
            &["endpoint_id", "path"],
        )
        .expect("Failed to create days_until_sunset metric");

        let request_duration_seconds = HistogramVec::new(
            prometheus::HistogramOpts::new(
                format!("{}_request_duration_seconds", prefix),
                "Request duration for deprecated endpoints",
            )
            .buckets(vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0]),
            &["endpoint_id"],
        )
        .expect("Failed to create request_duration_seconds metric");

        // Register all metrics
        registry
            .register(Box::new(requests_total.clone()))
            .expect("Failed to register requests_total");
        registry
            .register(Box::new(redirects_total.clone()))
            .expect("Failed to register redirects_total");
        registry
            .register(Box::new(blocked_total.clone()))
            .expect("Failed to register blocked_total");
        registry
            .register(Box::new(days_until_sunset.clone()))
            .expect("Failed to register days_until_sunset");
        registry
            .register(Box::new(request_duration_seconds.clone()))
            .expect("Failed to register request_duration_seconds");

        Self {
            registry,
            requests_total,
            redirects_total,
            blocked_total,
            days_until_sunset,
            request_duration_seconds,
        }
    }

    /// Record a request to a deprecated endpoint.
    pub fn record_request(
        &self,
        endpoint_id: &str,
        path: &str,
        method: &str,
        status: &str,
    ) {
        self.requests_total
            .with_label_values(&[endpoint_id, path, method, status])
            .inc();
    }

    /// Record a redirect from a deprecated endpoint.
    pub fn record_redirect(&self, endpoint_id: &str, from_path: &str, to_path: &str) {
        self.redirects_total
            .with_label_values(&[endpoint_id, from_path, to_path])
            .inc();
    }

    /// Record a blocked request.
    pub fn record_blocked(&self, endpoint_id: &str, path: &str, reason: &str) {
        self.blocked_total
            .with_label_values(&[endpoint_id, path, reason])
            .inc();
    }

    /// Update the days until sunset gauge.
    pub fn set_days_until_sunset(&self, endpoint_id: &str, path: &str, days: i64) {
        self.days_until_sunset
            .with_label_values(&[endpoint_id, path])
            .set(days);
    }

    /// Record request duration.
    pub fn observe_duration(&self, endpoint_id: &str, duration_secs: f64) {
        self.request_duration_seconds
            .with_label_values(&[endpoint_id])
            .observe(duration_secs);
    }

    /// Get the Prometheus registry.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Encode metrics in Prometheus text format.
    pub fn encode(&self) -> String {
        use prometheus::Encoder;
        let encoder = prometheus::TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        String::from_utf8(buffer).unwrap()
    }
}

impl Default for DeprecationMetrics {
    fn default() -> Self {
        Self::new("zentinel_api_deprecation")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation() {
        let metrics = DeprecationMetrics::new("test");
        // Record a value to initialize the metric
        metrics.record_request("test-endpoint", "/test", "GET", "deprecated");
        assert!(!metrics.encode().is_empty());
    }

    #[test]
    fn test_record_request() {
        let metrics = DeprecationMetrics::new("test");
        metrics.record_request("legacy-api", "/api/v1/users", "GET", "deprecated");

        let output = metrics.encode();
        assert!(output.contains("test_requests_total"));
        assert!(output.contains("legacy-api"));
    }

    #[test]
    fn test_record_redirect() {
        let metrics = DeprecationMetrics::new("test");
        metrics.record_redirect("legacy-api", "/api/v1/users", "/api/v2/users");

        let output = metrics.encode();
        assert!(output.contains("test_redirects_total"));
    }

    #[test]
    fn test_days_until_sunset() {
        let metrics = DeprecationMetrics::new("test");
        metrics.set_days_until_sunset("legacy-api", "/api/v1/users", 30);

        let output = metrics.encode();
        assert!(output.contains("test_days_until_sunset"));
        assert!(output.contains("30"));
    }
}
