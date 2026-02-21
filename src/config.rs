//! Configuration for the API Deprecation agent.
//!
//! Defines deprecated endpoints, sunset dates, redirect rules, and tracking options.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Main configuration for the API Deprecation agent.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiDeprecationConfig {
    /// List of deprecated endpoints
    #[serde(default)]
    pub endpoints: Vec<DeprecatedEndpoint>,

    /// Global settings
    #[serde(default)]
    pub settings: GlobalSettings,

    /// Metrics configuration
    #[serde(default)]
    pub metrics: MetricsConfig,
}

impl ApiDeprecationConfig {
    /// Load configuration from a YAML file.
    pub fn from_file(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = serde_yaml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    /// Validate the configuration.
    pub fn validate(&self) -> anyhow::Result<()> {
        for endpoint in &self.endpoints {
            endpoint.validate()?;
        }
        Ok(())
    }

    /// Find a matching deprecated endpoint for a given path and method.
    pub fn find_endpoint(&self, path: &str, method: &str) -> Option<&DeprecatedEndpoint> {
        self.endpoints.iter().find(|e| e.matches(path, method))
    }
}

/// Configuration for a single deprecated endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeprecatedEndpoint {
    /// Unique identifier for this deprecation rule
    pub id: String,

    /// Path pattern to match (supports glob patterns like /api/v1/*)
    pub path: String,

    /// HTTP methods to match (empty means all methods)
    #[serde(default)]
    pub methods: Vec<String>,

    /// Deprecation status
    #[serde(default)]
    pub status: DeprecationStatus,

    /// Date when the endpoint was deprecated (RFC 3339)
    #[serde(default)]
    pub deprecated_at: Option<DateTime<Utc>>,

    /// Date when the endpoint will be/was removed (RFC 3339)
    /// Used for the Sunset header (RFC 8594)
    #[serde(default)]
    pub sunset_at: Option<DateTime<Utc>>,

    /// Replacement endpoint information
    #[serde(default)]
    pub replacement: Option<ReplacementInfo>,

    /// Link to migration documentation
    #[serde(default)]
    pub documentation_url: Option<String>,

    /// Custom deprecation message
    #[serde(default)]
    pub message: Option<String>,

    /// Action to take when this endpoint is accessed
    #[serde(default)]
    pub action: DeprecationAction,

    /// Additional headers to add to responses
    #[serde(default)]
    pub headers: HashMap<String, String>,

    /// Whether to track usage of this endpoint
    #[serde(default = "default_true")]
    pub track_usage: bool,

    /// Compiled path matcher (not serialized)
    #[serde(skip)]
    pub path_matcher: Option<globset::GlobMatcher>,
}

fn default_true() -> bool {
    true
}

impl DeprecatedEndpoint {
    /// Validate the endpoint configuration.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.id.is_empty() {
            anyhow::bail!("Endpoint id cannot be empty");
        }
        if self.path.is_empty() {
            anyhow::bail!("Endpoint path cannot be empty for id: {}", self.id);
        }

        // Validate sunset date is in the future for non-removed endpoints
        if let (Some(sunset), DeprecationStatus::Deprecated) = (&self.sunset_at, &self.status) {
            if *sunset < Utc::now() {
                tracing::warn!(
                    endpoint_id = %self.id,
                    sunset = %sunset,
                    "Sunset date is in the past but status is still 'deprecated'"
                );
            }
        }

        // Validate redirect has a target
        if matches!(self.action, DeprecationAction::Redirect { .. })
            && self.replacement.is_none()
        {
            anyhow::bail!(
                "Redirect action requires replacement info for endpoint: {}",
                self.id
            );
        }

        Ok(())
    }

    /// Check if this endpoint matches the given path and method.
    pub fn matches(&self, path: &str, method: &str) -> bool {
        // Check method first (quick check)
        if !self.methods.is_empty() {
            let method_upper = method.to_uppercase();
            if !self.methods.iter().any(|m| m.to_uppercase() == method_upper) {
                return false;
            }
        }

        // Check path pattern
        self.matches_path(path)
    }

    /// Check if the path matches the pattern.
    fn matches_path(&self, path: &str) -> bool {
        // Simple prefix/exact matching for common cases
        if !self.path.contains('*') && !self.path.contains('?') {
            // Exact match or prefix match with trailing slash
            return path == self.path
                || path.starts_with(&format!("{}/", self.path))
                || (self.path.ends_with('/') && path.starts_with(&self.path));
        }

        // Use glob matching for patterns
        if let Ok(glob) = globset::Glob::new(&self.path) {
            let matcher = glob.compile_matcher();
            return matcher.is_match(path);
        }

        false
    }

    /// Check if the endpoint has passed its sunset date.
    pub fn is_past_sunset(&self) -> bool {
        self.sunset_at
            .map(|sunset| Utc::now() > sunset)
            .unwrap_or(false)
    }

    /// Get the deprecation warning message.
    pub fn deprecation_message(&self) -> String {
        if let Some(msg) = &self.message {
            return msg.clone();
        }

        let mut message = format!("This endpoint ({}) is deprecated", self.path);

        if let Some(sunset) = &self.sunset_at {
            message.push_str(&format!(" and will be removed on {}", sunset.format("%Y-%m-%d")));
        }

        if let Some(replacement) = &self.replacement {
            message.push_str(&format!(". Please migrate to {}", replacement.path));
        }

        if let Some(docs) = &self.documentation_url {
            message.push_str(&format!(". See {} for migration guide", docs));
        }

        message.push('.');
        message
    }
}

/// Status of the deprecation.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DeprecationStatus {
    /// Endpoint is deprecated but still functional
    #[default]
    Deprecated,
    /// Endpoint is removed and should return an error
    Removed,
    /// Endpoint is scheduled for deprecation (warning only)
    Scheduled,
}

/// Information about the replacement endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplacementInfo {
    /// Path to the new endpoint
    pub path: String,

    /// Whether to preserve query parameters during redirect
    #[serde(default = "default_true")]
    pub preserve_query: bool,

    /// Path parameter mappings (old param name -> new param name)
    #[serde(default)]
    pub param_mappings: HashMap<String, String>,

    /// HTTP method for the new endpoint (if different)
    #[serde(default)]
    pub method: Option<String>,
}

/// Action to take when a deprecated endpoint is accessed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum DeprecationAction {
    /// Allow the request but add deprecation headers
    #[default]
    Warn,

    /// Redirect to the replacement endpoint
    Redirect {
        /// HTTP status code for redirect (default: 308 Permanent Redirect)
        #[serde(default = "default_redirect_code")]
        status_code: u16,
    },

    /// Block the request with an error response
    Block {
        /// HTTP status code (default: 410 Gone)
        #[serde(default = "default_gone_code")]
        status_code: u16,
    },

    /// Custom response
    Custom {
        /// HTTP status code
        status_code: u16,
        /// Response body
        body: String,
        /// Content-Type header
        #[serde(default = "default_content_type")]
        content_type: String,
    },
}

fn default_redirect_code() -> u16 {
    308
}

fn default_gone_code() -> u16 {
    410
}

fn default_content_type() -> String {
    "application/json".to_string()
}

/// Global settings for the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalSettings {
    /// Header name for deprecation warnings (default: Deprecation)
    #[serde(default = "default_deprecation_header")]
    pub deprecation_header: String,

    /// Header name for sunset date (default: Sunset)
    #[serde(default = "default_sunset_header")]
    pub sunset_header: String,

    /// Header name for link to documentation (default: Link)
    #[serde(default = "default_link_header")]
    pub link_header: String,

    /// Header name for deprecation message (default: X-Deprecation-Notice)
    #[serde(default = "default_notice_header")]
    pub notice_header: String,

    /// Whether to include deprecation headers on all matching requests
    #[serde(default = "default_true")]
    pub include_headers: bool,

    /// Default action for endpoints past their sunset date
    #[serde(default)]
    pub past_sunset_action: PastSunsetAction,

    /// Whether to log all deprecated endpoint access
    #[serde(default = "default_true")]
    pub log_access: bool,
}

impl Default for GlobalSettings {
    fn default() -> Self {
        Self {
            deprecation_header: default_deprecation_header(),
            sunset_header: default_sunset_header(),
            link_header: default_link_header(),
            notice_header: default_notice_header(),
            include_headers: true,
            past_sunset_action: PastSunsetAction::default(),
            log_access: true,
        }
    }
}

fn default_deprecation_header() -> String {
    "Deprecation".to_string()
}

fn default_sunset_header() -> String {
    "Sunset".to_string()
}

fn default_link_header() -> String {
    "Link".to_string()
}

fn default_notice_header() -> String {
    "X-Deprecation-Notice".to_string()
}

/// Action to take when an endpoint is accessed past its sunset date.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PastSunsetAction {
    /// Continue to allow with headers (default)
    #[default]
    Warn,
    /// Block with 410 Gone
    Block,
    /// Redirect to replacement (if available)
    Redirect,
}

/// Metrics configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricsConfig {
    /// Whether to expose Prometheus metrics
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Prefix for metric names
    #[serde(default = "default_metrics_prefix")]
    pub prefix: String,

    /// Labels to include in metrics
    #[serde(default)]
    pub labels: HashMap<String, String>,

    /// Port for metrics endpoint (0 = disabled)
    #[serde(default)]
    pub port: u16,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            prefix: default_metrics_prefix(),
            labels: HashMap::new(),
            port: 0,
        }
    }
}

fn default_metrics_prefix() -> String {
    "zentinel_api_deprecation".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic_config() {
        let yaml = r#"
endpoints:
  - id: legacy-users-api
    path: /api/v1/users
    methods: [GET, POST]
    status: deprecated
    sunset_at: "2025-06-01T00:00:00Z"
    replacement:
      path: /api/v2/users
    documentation_url: https://docs.example.com/migration/users
    message: "Please migrate to the v2 API"
"#;
        let config: ApiDeprecationConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.endpoints.len(), 1);
        assert_eq!(config.endpoints[0].id, "legacy-users-api");
        assert_eq!(config.endpoints[0].path, "/api/v1/users");
        assert_eq!(config.endpoints[0].methods, vec!["GET", "POST"]);
    }

    #[test]
    fn test_endpoint_matching() {
        let endpoint = DeprecatedEndpoint {
            id: "test".to_string(),
            path: "/api/v1/users".to_string(),
            methods: vec!["GET".to_string()],
            status: DeprecationStatus::Deprecated,
            deprecated_at: None,
            sunset_at: None,
            replacement: None,
            documentation_url: None,
            message: None,
            action: DeprecationAction::Warn,
            headers: HashMap::new(),
            track_usage: true,
            path_matcher: None,
        };

        assert!(endpoint.matches("/api/v1/users", "GET"));
        assert!(endpoint.matches("/api/v1/users", "get"));
        assert!(!endpoint.matches("/api/v1/users", "POST"));
        assert!(!endpoint.matches("/api/v2/users", "GET"));
    }

    #[test]
    fn test_glob_pattern_matching() {
        let endpoint = DeprecatedEndpoint {
            id: "test".to_string(),
            path: "/api/v1/*".to_string(),
            methods: vec![],
            status: DeprecationStatus::Deprecated,
            deprecated_at: None,
            sunset_at: None,
            replacement: None,
            documentation_url: None,
            message: None,
            action: DeprecationAction::Warn,
            headers: HashMap::new(),
            track_usage: true,
            path_matcher: None,
        };

        assert!(endpoint.matches("/api/v1/users", "GET"));
        assert!(endpoint.matches("/api/v1/posts", "POST"));
        assert!(!endpoint.matches("/api/v2/users", "GET"));
    }

    #[test]
    fn test_deprecation_action_redirect() {
        let yaml = r#"
type: redirect
status_code: 301
"#;
        let action: DeprecationAction = serde_yaml::from_str(yaml).unwrap();
        match action {
            DeprecationAction::Redirect { status_code } => {
                assert_eq!(status_code, 301);
            }
            _ => assert!(false, "Expected Redirect action"),
        }
    }

    #[test]
    fn test_deprecation_message() {
        let endpoint = DeprecatedEndpoint {
            id: "test".to_string(),
            path: "/api/v1/users".to_string(),
            methods: vec![],
            status: DeprecationStatus::Deprecated,
            deprecated_at: None,
            sunset_at: Some("2025-06-01T00:00:00Z".parse().unwrap()),
            replacement: Some(ReplacementInfo {
                path: "/api/v2/users".to_string(),
                preserve_query: true,
                param_mappings: HashMap::new(),
                method: None,
            }),
            documentation_url: Some("https://docs.example.com".to_string()),
            message: None,
            action: DeprecationAction::Warn,
            headers: HashMap::new(),
            track_usage: true,
            path_matcher: None,
        };

        let msg = endpoint.deprecation_message();
        assert!(msg.contains("/api/v1/users"));
        assert!(msg.contains("2025-06-01"));
        assert!(msg.contains("/api/v2/users"));
        assert!(msg.contains("docs.example.com"));
    }

    #[test]
    fn test_custom_message() {
        let endpoint = DeprecatedEndpoint {
            id: "test".to_string(),
            path: "/api/v1/users".to_string(),
            methods: vec![],
            status: DeprecationStatus::Deprecated,
            deprecated_at: None,
            sunset_at: None,
            replacement: None,
            documentation_url: None,
            message: Some("Custom deprecation message".to_string()),
            action: DeprecationAction::Warn,
            headers: HashMap::new(),
            track_usage: true,
            path_matcher: None,
        };

        assert_eq!(endpoint.deprecation_message(), "Custom deprecation message");
    }
}
