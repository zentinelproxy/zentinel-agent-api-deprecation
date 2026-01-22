//! Main API Deprecation agent implementation.

use crate::config::{
    ApiDeprecationConfig, DeprecatedEndpoint, DeprecationAction, DeprecationStatus,
    PastSunsetAction,
};
use crate::headers::{gone_response_body, DeprecationHeaders};
use crate::metrics::DeprecationMetrics;
use async_trait::async_trait;
use chrono::Utc;
use sentinel_agent_sdk::{Agent, Decision, Request, Response};
use sentinel_agent_protocol::v2::{
    AgentCapabilities, AgentFeatures, AgentHandlerV2, CounterMetric, DrainReason, GaugeMetric,
    HealthStatus, MetricsReport, ShutdownReason,
};
use sentinel_agent_protocol::{AgentResponse, EventType, RequestHeadersEvent, ResponseHeadersEvent};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// API Deprecation Agent
///
/// Manages API lifecycle by adding deprecation headers, tracking usage,
/// and handling sunset policies for deprecated endpoints.
pub struct ApiDeprecationAgent {
    config: ApiDeprecationConfig,
    metrics: Arc<DeprecationMetrics>,
    /// Whether the agent is draining (not accepting new requests)
    draining: AtomicBool,
}

impl ApiDeprecationAgent {
    /// Create a new API deprecation agent with the given configuration.
    pub fn new(config: ApiDeprecationConfig) -> Self {
        let metrics = Arc::new(DeprecationMetrics::new(&config.metrics.prefix));

        // Initialize days_until_sunset metrics for all endpoints
        for endpoint in &config.endpoints {
            if let Some(sunset) = &endpoint.sunset_at {
                let days = (*sunset - Utc::now()).num_days();
                metrics.set_days_until_sunset(&endpoint.id, &endpoint.path, days);
            }
        }

        info!(
            endpoints = config.endpoints.len(),
            "API deprecation agent initialized"
        );

        Self {
            config,
            metrics,
            draining: AtomicBool::new(false),
        }
    }

    /// Create from a YAML configuration string.
    pub fn from_yaml(yaml: &str) -> Result<Self, serde_yaml::Error> {
        let config: ApiDeprecationConfig = serde_yaml::from_str(yaml)?;
        Ok(Self::new(config))
    }

    /// Get the metrics collector.
    pub fn metrics(&self) -> &DeprecationMetrics {
        &self.metrics
    }

    /// Process a request and determine the appropriate action.
    fn process_request(
        &self,
        path: &str,
        method: &str,
        query_string: Option<&str>,
    ) -> Option<DeprecationDecision> {
        // Find matching deprecated endpoint
        let endpoint = self.config.find_endpoint(path, method)?;

        debug!(
            endpoint_id = %endpoint.id,
            path = %path,
            method = %method,
            "Request matches deprecated endpoint"
        );

        // Track usage
        if endpoint.track_usage {
            let status = match endpoint.status {
                DeprecationStatus::Deprecated => "deprecated",
                DeprecationStatus::Removed => "removed",
                DeprecationStatus::Scheduled => "scheduled",
            };
            self.metrics
                .record_request(&endpoint.id, path, method, status);
        }

        // Check if past sunset
        let past_sunset = endpoint.is_past_sunset();
        if past_sunset {
            warn!(
                endpoint_id = %endpoint.id,
                sunset = ?endpoint.sunset_at,
                "Request to endpoint past sunset date"
            );
        }

        // Determine action
        let action = self.determine_action(endpoint, past_sunset);

        // Build deprecation headers
        let headers = DeprecationHeaders::for_endpoint(endpoint, &self.config.settings).build();

        // Build redirect URL if needed
        let redirect_url = if matches!(action, DeprecationActionResult::Redirect { .. }) {
            endpoint.replacement.as_ref().map(|r| {
                let mut url = r.path.clone();
                if r.preserve_query {
                    if let Some(qs) = query_string {
                        if !qs.is_empty() {
                            url.push('?');
                            url.push_str(qs);
                        }
                    }
                }
                url
            })
        } else {
            None
        };

        Some(DeprecationDecision {
            endpoint_id: endpoint.id.clone(),
            action,
            headers,
            redirect_url,
            message: endpoint.deprecation_message(),
            documentation_url: endpoint.documentation_url.clone(),
        })
    }

    /// Determine the action to take based on endpoint config and sunset status.
    fn determine_action(
        &self,
        endpoint: &DeprecatedEndpoint,
        past_sunset: bool,
    ) -> DeprecationActionResult {
        // If removed, always block
        if matches!(endpoint.status, DeprecationStatus::Removed) {
            return DeprecationActionResult::Block { status_code: 410 };
        }

        // If past sunset, apply global policy
        if past_sunset {
            return match self.config.settings.past_sunset_action {
                PastSunsetAction::Warn => DeprecationActionResult::Warn,
                PastSunsetAction::Block => DeprecationActionResult::Block { status_code: 410 },
                PastSunsetAction::Redirect => {
                    if endpoint.replacement.is_some() {
                        DeprecationActionResult::Redirect { status_code: 301 }
                    } else {
                        DeprecationActionResult::Block { status_code: 410 }
                    }
                }
            };
        }

        // Otherwise, use endpoint-specific action
        match &endpoint.action {
            DeprecationAction::Warn => DeprecationActionResult::Warn,
            DeprecationAction::Redirect { status_code } => DeprecationActionResult::Redirect {
                status_code: *status_code,
            },
            DeprecationAction::Block { status_code } => DeprecationActionResult::Block {
                status_code: *status_code,
            },
            DeprecationAction::Custom {
                status_code,
                body,
                content_type,
            } => DeprecationActionResult::Custom {
                status_code: *status_code,
                body: body.clone(),
                content_type: content_type.clone(),
            },
        }
    }

    /// Apply deprecation headers to an allow decision.
    fn apply_headers(&self, decision: Decision, headers: HashMap<String, String>) -> Decision {
        let mut d = decision;
        for (name, value) in headers {
            d = d.add_response_header(name, value);
        }
        d
    }
}

/// Result of processing a deprecated endpoint.
struct DeprecationDecision {
    endpoint_id: String,
    action: DeprecationActionResult,
    headers: HashMap<String, String>,
    redirect_url: Option<String>,
    message: String,
    documentation_url: Option<String>,
}

/// Action result after processing.
#[derive(Debug, Clone)]
enum DeprecationActionResult {
    Warn,
    Redirect { status_code: u16 },
    Block { status_code: u16 },
    Custom {
        status_code: u16,
        body: String,
        content_type: String,
    },
}

// The agent needs to be Send + Sync for the SDK
unsafe impl Send for ApiDeprecationAgent {}
unsafe impl Sync for ApiDeprecationAgent {}

#[async_trait]
impl Agent for ApiDeprecationAgent {
    async fn on_request(&self, request: &Request) -> Decision {
        let method = request.method();
        let path = request.path();
        let query_string = request.query_string();

        // Process the request
        let decision = match self.process_request(path, method, query_string) {
            Some(d) => d,
            None => {
                // Not a deprecated endpoint, allow
                return Decision::allow();
            }
        };

        // Log the access
        if self.config.settings.log_access {
            info!(
                endpoint_id = %decision.endpoint_id,
                path = %path,
                method = %method,
                action = ?decision.action,
                "Deprecated endpoint accessed"
            );
        }

        // Apply the action
        match decision.action {
            DeprecationActionResult::Warn => {
                // Allow but add deprecation headers
                let mut d = Decision::allow()
                    .with_tag("deprecated")
                    .with_metadata("deprecated_endpoint", serde_json::json!(decision.endpoint_id));

                d = self.apply_headers(d, decision.headers);
                d
            }

            DeprecationActionResult::Redirect { status_code } => {
                if let Some(redirect_url) = decision.redirect_url {
                    self.metrics.record_redirect(
                        &decision.endpoint_id,
                        path,
                        &redirect_url,
                    );

                    // Use permanent redirect for 301, regular for others
                    // Note: SDK supports 301 and 302; for 308 we use block with Location header
                    let mut d = if status_code == 301 {
                        Decision::redirect_permanent(&redirect_url)
                    } else if status_code == 302 {
                        Decision::redirect(&redirect_url)
                    } else {
                        // For 308 or other codes, use block with Location header
                        Decision::block(status_code)
                            .with_block_header("Location", &redirect_url)
                            .with_body("")
                    };

                    d = d
                        .with_tag("deprecated")
                        .with_tag("redirected")
                        .with_metadata("deprecated_endpoint", serde_json::json!(decision.endpoint_id))
                        .with_metadata("redirect_target", serde_json::json!(redirect_url));

                    // Add deprecation headers to the redirect response
                    for (name, value) in decision.headers {
                        d = d.with_block_header(name, value);
                    }

                    d
                } else {
                    // No replacement URL, block instead
                    self.metrics
                        .record_blocked(&decision.endpoint_id, path, "no_replacement");

                    Decision::block(410)
                        .with_body(gone_response_body(&DeprecatedEndpoint {
                            id: decision.endpoint_id.clone(),
                            path: path.to_string(),
                            methods: vec![],
                            status: DeprecationStatus::Removed,
                            deprecated_at: None,
                            sunset_at: None,
                            replacement: None,
                            documentation_url: decision.documentation_url,
                            message: Some(decision.message),
                            action: DeprecationAction::Block { status_code: 410 },
                            headers: HashMap::new(),
                            track_usage: false,
                            path_matcher: None,
                        }))
                        .with_block_header("Content-Type", "application/json")
                        .with_tag("deprecated")
                        .with_tag("blocked")
                }
            }

            DeprecationActionResult::Block { status_code } => {
                self.metrics
                    .record_blocked(&decision.endpoint_id, path, "removed");

                let body = gone_response_body(&DeprecatedEndpoint {
                    id: decision.endpoint_id.clone(),
                    path: path.to_string(),
                    methods: vec![],
                    status: DeprecationStatus::Removed,
                    deprecated_at: None,
                    sunset_at: None,
                    replacement: None,
                    documentation_url: decision.documentation_url,
                    message: Some(decision.message),
                    action: DeprecationAction::Block { status_code },
                    headers: HashMap::new(),
                    track_usage: false,
                    path_matcher: None,
                });

                let mut d = Decision::block(status_code)
                    .with_body(body)
                    .with_block_header("Content-Type", "application/json")
                    .with_tag("deprecated")
                    .with_tag("blocked")
                    .with_metadata("deprecated_endpoint", serde_json::json!(decision.endpoint_id));

                // Add deprecation headers
                for (name, value) in decision.headers {
                    d = d.with_block_header(name, value);
                }

                d
            }

            DeprecationActionResult::Custom {
                status_code,
                body,
                content_type,
            } => {
                Decision::block(status_code)
                    .with_body(body)
                    .with_block_header("Content-Type", content_type)
                    .with_tag("deprecated")
                    .with_tag("custom_response")
                    .with_metadata("deprecated_endpoint", serde_json::json!(decision.endpoint_id))
            }
        }
    }

    async fn on_response(&self, _request: &Request, _response: &Response) -> Decision {
        // Response phase - nothing to do for deprecation
        // Headers are already added in on_request for allowed requests
        Decision::allow()
    }
}

/// Protocol v2 implementation for API Deprecation Agent.
///
/// Provides capability negotiation, health reporting, metrics export,
/// and lifecycle management.
#[async_trait]
impl AgentHandlerV2 for ApiDeprecationAgent {
    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities::new(
            "api-deprecation",
            "API Deprecation Agent",
            env!("CARGO_PKG_VERSION"),
        )
        .with_event(EventType::RequestHeaders)
        .with_event(EventType::ResponseHeaders)
        .with_features(AgentFeatures {
            streaming_body: false,
            config_push: true,
            health_reporting: true,
            metrics_export: true,
            concurrent_requests: 100,
            cancellation: false,
            flow_control: false,
            max_processing_time_ms: 1000,
            health_interval_ms: 10000,
        })
    }

    fn health_status(&self) -> HealthStatus {
        if self.draining.load(Ordering::Relaxed) {
            HealthStatus::degraded(
                "api-deprecation",
                vec!["new_requests".to_string()],
                1.0,
            )
        } else {
            HealthStatus::healthy("api-deprecation")
        }
    }

    fn metrics_report(&self) -> Option<MetricsReport> {
        let mut report = MetricsReport::new("api-deprecation", 10000);

        // Add endpoint count gauge
        report.gauges.push(GaugeMetric::new(
            "api_deprecation_endpoints_total",
            self.config.endpoints.len() as f64,
        ));

        // Add counters for each endpoint's days until sunset
        for endpoint in &self.config.endpoints {
            if let Some(sunset) = &endpoint.sunset_at {
                let days = (*sunset - Utc::now()).num_days();
                let mut metric = GaugeMetric::new(
                    "api_deprecation_days_until_sunset",
                    days as f64,
                );
                metric.labels.insert("endpoint_id".to_string(), endpoint.id.clone());
                metric.labels.insert("path".to_string(), endpoint.path.clone());
                report.gauges.push(metric);
            }
        }

        // Add request counters from our Prometheus metrics (if we have any recorded)
        // Note: In a real implementation, we'd aggregate from self.metrics
        // For now, we just report the endpoint configuration

        if report.is_empty() {
            None
        } else {
            Some(report)
        }
    }

    async fn on_shutdown(&self, reason: ShutdownReason, grace_period_ms: u64) {
        info!(
            ?reason,
            grace_period_ms,
            "API deprecation agent shutting down"
        );
        self.draining.store(true, Ordering::Relaxed);
    }

    async fn on_drain(&self, duration_ms: u64, reason: DrainReason) {
        info!(
            ?reason,
            duration_ms,
            "API deprecation agent draining"
        );
        self.draining.store(true, Ordering::Relaxed);
    }

    fn on_stream_closed(&self) {
        debug!("API deprecation agent stream closed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> ApiDeprecationConfig {
        let yaml = r#"
endpoints:
  - id: legacy-users
    path: /api/v1/users
    methods: [GET, POST]
    status: deprecated
    sunset_at: "2030-06-01T00:00:00Z"
    replacement:
      path: /api/v2/users
    documentation_url: https://docs.example.com/migration
    action:
      type: warn

  - id: removed-posts
    path: /api/v1/posts
    status: removed
    action:
      type: block
      status_code: 410

  - id: redirect-orders
    path: /api/v1/orders
    status: deprecated
    replacement:
      path: /api/v2/orders
    action:
      type: redirect
      status_code: 308
"#;
        serde_yaml::from_str(yaml).unwrap()
    }

    #[test]
    fn test_agent_creation() {
        let config = test_config();
        let agent = ApiDeprecationAgent::new(config);
        assert_eq!(agent.config.endpoints.len(), 3);
    }

    #[test]
    fn test_process_deprecated_endpoint() {
        let config = test_config();
        let agent = ApiDeprecationAgent::new(config);

        let decision = agent.process_request("/api/v1/users", "GET", None);
        assert!(decision.is_some());

        let d = decision.unwrap();
        assert_eq!(d.endpoint_id, "legacy-users");
        assert!(matches!(d.action, DeprecationActionResult::Warn));
    }

    #[test]
    fn test_process_removed_endpoint() {
        let config = test_config();
        let agent = ApiDeprecationAgent::new(config);

        let decision = agent.process_request("/api/v1/posts", "GET", None);
        assert!(decision.is_some());

        let d = decision.unwrap();
        assert_eq!(d.endpoint_id, "removed-posts");
        assert!(matches!(d.action, DeprecationActionResult::Block { status_code: 410 }));
    }

    #[test]
    fn test_process_redirect_endpoint() {
        let config = test_config();
        let agent = ApiDeprecationAgent::new(config);

        let decision = agent.process_request("/api/v1/orders", "GET", Some("page=1"));
        assert!(decision.is_some());

        let d = decision.unwrap();
        assert_eq!(d.endpoint_id, "redirect-orders");
        assert!(matches!(d.action, DeprecationActionResult::Redirect { status_code: 308 }));
        assert_eq!(d.redirect_url, Some("/api/v2/orders?page=1".to_string()));
    }

    #[test]
    fn test_non_deprecated_endpoint() {
        let config = test_config();
        let agent = ApiDeprecationAgent::new(config);

        let decision = agent.process_request("/api/v2/users", "GET", None);
        assert!(decision.is_none());
    }

    #[test]
    fn test_method_filtering() {
        let config = test_config();
        let agent = ApiDeprecationAgent::new(config);

        // GET should match
        let decision = agent.process_request("/api/v1/users", "GET", None);
        assert!(decision.is_some());

        // DELETE should not match (only GET, POST configured)
        let decision = agent.process_request("/api/v1/users", "DELETE", None);
        assert!(decision.is_none());
    }

    #[test]
    fn test_deprecation_headers() {
        let config = test_config();
        let agent = ApiDeprecationAgent::new(config);

        let decision = agent.process_request("/api/v1/users", "GET", None).unwrap();

        // Check that deprecation headers are present
        assert!(decision.headers.contains_key("Deprecation"));
        assert!(decision.headers.contains_key("Sunset"));
        assert!(decision.headers.contains_key("Link"));
        assert!(decision.headers.contains_key("X-Deprecation-Notice"));
    }

    #[test]
    fn test_metrics_tracking() {
        let config = test_config();
        let agent = ApiDeprecationAgent::new(config);

        // Make a request
        let _ = agent.process_request("/api/v1/users", "GET", None);

        // Check metrics were recorded
        let output = agent.metrics().encode();
        assert!(output.contains("requests_total"));
        assert!(output.contains("legacy-users"));
    }
}
