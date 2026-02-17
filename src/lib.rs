//! Zentinel API Deprecation Agent
//!
//! Manages API lifecycle by adding deprecation and sunset headers, tracking
//! usage of deprecated endpoints, and handling migration through redirects.
//!
//! # Features
//!
//! - **Sunset Headers**: RFC 8594 compliant Sunset headers
//! - **Deprecation Headers**: Standard deprecation warnings
//! - **Usage Tracking**: Prometheus metrics for deprecated endpoint usage
//! - **Automatic Redirects**: Redirect deprecated endpoints to replacements
//! - **Gradual Migration**: Configure different actions per endpoint
//!
//! # Example Configuration
//!
//! ```yaml
//! endpoints:
//!   - id: legacy-users-api
//!     path: /api/v1/users
//!     methods: [GET, POST]
//!     status: deprecated
//!     sunset_at: "2025-06-01T00:00:00Z"
//!     replacement:
//!       path: /api/v2/users
//!     documentation_url: https://docs.example.com/migration
//!     action:
//!       type: warn
//! ```

pub mod agent;
pub mod config;
pub mod headers;
pub mod metrics;

pub use agent::ApiDeprecationAgent;
pub use config::ApiDeprecationConfig;
