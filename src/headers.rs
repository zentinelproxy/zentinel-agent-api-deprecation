//! Header generation for API deprecation.
//!
//! Implements standard headers for API deprecation:
//! - Deprecation header (draft-ietf-httpapi-deprecation-header)
//! - Sunset header (RFC 8594)
//! - Link header with documentation

use crate::config::{DeprecatedEndpoint, GlobalSettings};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Builder for deprecation-related HTTP headers.
pub struct DeprecationHeaders {
    headers: HashMap<String, String>,
}

impl DeprecationHeaders {
    /// Create a new header builder.
    pub fn new() -> Self {
        Self {
            headers: HashMap::new(),
        }
    }

    /// Build headers for a deprecated endpoint.
    pub fn for_endpoint(endpoint: &DeprecatedEndpoint, settings: &GlobalSettings) -> Self {
        let mut builder = Self::new();

        // Add Deprecation header (draft-ietf-httpapi-deprecation-header)
        // Format: Deprecation: true or Deprecation: @timestamp
        if let Some(deprecated_at) = &endpoint.deprecated_at {
            builder.headers.insert(
                settings.deprecation_header.clone(),
                format!("@{}", deprecated_at.timestamp()),
            );
        } else {
            builder
                .headers
                .insert(settings.deprecation_header.clone(), "true".to_string());
        }

        // Add Sunset header (RFC 8594)
        // Format: Sunset: <HTTP-date>
        if let Some(sunset_at) = &endpoint.sunset_at {
            builder
                .headers
                .insert(settings.sunset_header.clone(), format_http_date(sunset_at));
        }

        // Add Link header for documentation
        if let Some(docs_url) = &endpoint.documentation_url {
            let link_value = format!("<{}>; rel=\"deprecation\"", docs_url);
            builder
                .headers
                .insert(settings.link_header.clone(), link_value);
        }

        // Add replacement link if available
        if let Some(replacement) = &endpoint.replacement {
            let existing_link = builder.headers.get(&settings.link_header).cloned();
            let successor_link = format!("<{}>; rel=\"successor-version\"", replacement.path);

            let link_value = match existing_link {
                Some(existing) => format!("{}, {}", existing, successor_link),
                None => successor_link,
            };
            builder
                .headers
                .insert(settings.link_header.clone(), link_value);
        }

        // Add deprecation notice message
        let message = endpoint.deprecation_message();
        builder
            .headers
            .insert(settings.notice_header.clone(), message);

        // Add any custom headers from the endpoint config
        for (key, value) in &endpoint.headers {
            builder.headers.insert(key.clone(), value.clone());
        }

        builder
    }

    /// Add a custom header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Get all headers.
    pub fn build(self) -> HashMap<String, String> {
        self.headers
    }

    /// Get headers as a vector of tuples.
    pub fn to_vec(self) -> Vec<(String, String)> {
        self.headers.into_iter().collect()
    }
}

impl Default for DeprecationHeaders {
    fn default() -> Self {
        Self::new()
    }
}

/// Format a datetime as an HTTP date (RFC 7231).
/// Example: Sun, 06 Nov 1994 08:49:37 GMT
fn format_http_date(dt: &DateTime<Utc>) -> String {
    dt.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

/// Parse an HTTP date to DateTime<Utc>.
pub fn parse_http_date(s: &str) -> Option<DateTime<Utc>> {
    use chrono::NaiveDateTime;

    // Try RFC 7231 format first (strip " GMT" suffix and parse as naive, then add UTC)
    if let Some(without_tz) = s.strip_suffix(" GMT") {
        if let Ok(naive) = NaiveDateTime::parse_from_str(without_tz, "%a, %d %b %Y %H:%M:%S") {
            return Some(naive.and_utc());
        }
    }

    // Try ISO 8601 as fallback
    if let Ok(dt) = s.parse::<DateTime<Utc>>() {
        return Some(dt);
    }

    None
}

/// Generate a standard deprecation response body.
pub fn deprecation_response_body(endpoint: &DeprecatedEndpoint) -> String {
    let mut response = serde_json::json!({
        "error": "deprecated_endpoint",
        "message": endpoint.deprecation_message(),
        "endpoint": endpoint.path,
    });

    if let Some(sunset) = &endpoint.sunset_at {
        response["sunset"] = serde_json::Value::String(sunset.to_rfc3339());
    }

    if let Some(replacement) = &endpoint.replacement {
        response["replacement"] = serde_json::Value::String(replacement.path.clone());
    }

    if let Some(docs) = &endpoint.documentation_url {
        response["documentation"] = serde_json::Value::String(docs.clone());
    }

    serde_json::to_string_pretty(&response).unwrap_or_default()
}

/// Generate a "410 Gone" response body.
pub fn gone_response_body(endpoint: &DeprecatedEndpoint) -> String {
    let mut response = serde_json::json!({
        "error": "endpoint_removed",
        "message": format!("The endpoint {} has been removed", endpoint.path),
    });

    if let Some(replacement) = &endpoint.replacement {
        response["replacement"] = serde_json::Value::String(replacement.path.clone());
        response["message"] = serde_json::Value::String(format!(
            "The endpoint {} has been removed. Please use {} instead",
            endpoint.path, replacement.path
        ));
    }

    if let Some(docs) = &endpoint.documentation_url {
        response["documentation"] = serde_json::Value::String(docs.clone());
    }

    serde_json::to_string_pretty(&response).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DeprecationAction, DeprecationStatus, ReplacementInfo};

    fn test_endpoint() -> DeprecatedEndpoint {
        DeprecatedEndpoint {
            id: "test".to_string(),
            path: "/api/v1/users".to_string(),
            methods: vec![],
            status: DeprecationStatus::Deprecated,
            deprecated_at: Some("2024-01-01T00:00:00Z".parse().unwrap()),
            sunset_at: Some("2025-06-01T00:00:00Z".parse().unwrap()),
            replacement: Some(ReplacementInfo {
                path: "/api/v2/users".to_string(),
                preserve_query: true,
                param_mappings: HashMap::new(),
                method: None,
            }),
            documentation_url: Some("https://docs.example.com/migration".to_string()),
            message: None,
            action: DeprecationAction::Warn,
            headers: HashMap::new(),
            track_usage: true,
            path_matcher: None,
        }
    }

    fn test_settings() -> GlobalSettings {
        GlobalSettings::default()
    }

    #[test]
    fn test_deprecation_header() {
        let endpoint = test_endpoint();
        let settings = test_settings();
        let headers = DeprecationHeaders::for_endpoint(&endpoint, &settings).build();

        assert!(headers.contains_key("Deprecation"));
        // Should contain Unix timestamp
        assert!(headers["Deprecation"].starts_with('@'));
    }

    #[test]
    fn test_sunset_header() {
        let endpoint = test_endpoint();
        let settings = test_settings();
        let headers = DeprecationHeaders::for_endpoint(&endpoint, &settings).build();

        assert!(headers.contains_key("Sunset"));
        // Should be in HTTP date format
        assert!(headers["Sunset"].contains("2025"));
        assert!(headers["Sunset"].ends_with("GMT"));
    }

    #[test]
    fn test_link_header() {
        let endpoint = test_endpoint();
        let settings = test_settings();
        let headers = DeprecationHeaders::for_endpoint(&endpoint, &settings).build();

        assert!(headers.contains_key("Link"));
        let link = &headers["Link"];
        assert!(link.contains("rel=\"deprecation\""));
        assert!(link.contains("rel=\"successor-version\""));
        assert!(link.contains("docs.example.com"));
        assert!(link.contains("/api/v2/users"));
    }

    #[test]
    fn test_notice_header() {
        let endpoint = test_endpoint();
        let settings = test_settings();
        let headers = DeprecationHeaders::for_endpoint(&endpoint, &settings).build();

        assert!(headers.contains_key("X-Deprecation-Notice"));
        let notice = &headers["X-Deprecation-Notice"];
        assert!(notice.contains("deprecated"));
    }

    #[test]
    fn test_format_http_date() {
        let dt: DateTime<Utc> = "2025-06-01T12:00:00Z".parse().unwrap();
        let formatted = format_http_date(&dt);
        assert!(formatted.contains("Jun"));
        assert!(formatted.contains("2025"));
        assert!(formatted.ends_with("GMT"));
    }

    #[test]
    fn test_parse_http_date() {
        let date_str = "Sun, 01 Jun 2025 12:00:00 GMT";
        let parsed = parse_http_date(date_str);
        assert!(parsed.is_some());

        // Also works with ISO 8601
        let iso_str = "2025-06-01T12:00:00Z";
        let parsed_iso = parse_http_date(iso_str);
        assert!(parsed_iso.is_some());
    }

    #[test]
    fn test_deprecation_response_body() {
        let endpoint = test_endpoint();
        let body = deprecation_response_body(&endpoint);

        assert!(body.contains("deprecated_endpoint"));
        assert!(body.contains("/api/v1/users"));
        assert!(body.contains("/api/v2/users"));
    }

    #[test]
    fn test_gone_response_body() {
        let endpoint = test_endpoint();
        let body = gone_response_body(&endpoint);

        assert!(body.contains("endpoint_removed"));
        assert!(body.contains("has been removed"));
    }
}
