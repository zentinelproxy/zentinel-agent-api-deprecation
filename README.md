# Zentinel API Deprecation Agent

An API lifecycle management agent for [Zentinel](https://zentinelproxy.io) that helps you gracefully deprecate and sunset API endpoints.

## Features

- **RFC-Compliant Headers**: Adds standard `Deprecation` and `Sunset` (RFC 8594) headers
- **Usage Tracking**: Prometheus metrics for monitoring deprecated endpoint usage
- **Flexible Actions**: Warn, redirect, or block deprecated endpoints
- **Automatic Redirects**: Redirect old endpoints to new versions with query preservation
- **Migration Support**: Include documentation links in deprecation notices
- **Glob Patterns**: Match multiple endpoints with glob-style patterns

## Installation

### Using Cargo

```bash
cargo install zentinel-agent-api-deprecation
```

### From Source

```bash
git clone https://github.com/zentinelproxy/zentinel-agent-api-deprecation
cd zentinel-agent-api-deprecation
cargo build --release
```

## Quick Start

1. Create a configuration file `api-deprecation.yaml`:

```yaml
endpoints:
  - id: legacy-users-api
    path: /api/v1/users
    methods: [GET, POST]
    status: deprecated
    sunset_at: "2025-06-01T00:00:00Z"
    replacement:
      path: /api/v2/users
    documentation_url: https://docs.example.com/migration
    action:
      type: warn
```

2. Add to your Zentinel configuration:

```kdl
agents {
    api-deprecation socket="/tmp/zentinel-api-deprecation.sock"
}
```

3. Start the agent:

```bash
zentinel-api-deprecation-agent -c api-deprecation.yaml
```

## Configuration

### Deprecated Endpoints

Each deprecated endpoint supports:

| Field | Type | Description |
|-------|------|-------------|
| `id` | string | Unique identifier for the endpoint |
| `path` | string | Path pattern (supports globs like `/api/v1/*`) |
| `methods` | list | HTTP methods to match (empty = all) |
| `status` | enum | `deprecated`, `scheduled`, or `removed` |
| `deprecated_at` | datetime | When the endpoint was deprecated |
| `sunset_at` | datetime | When the endpoint will be removed |
| `replacement` | object | Replacement endpoint info |
| `documentation_url` | string | Link to migration guide |
| `message` | string | Custom deprecation message |
| `action` | object | What to do when accessed |
| `track_usage` | bool | Track usage metrics (default: true) |

### Actions

**Warn** (default): Allow the request but add deprecation headers
```yaml
action:
  type: warn
```

**Redirect**: Redirect to the replacement endpoint
```yaml
action:
  type: redirect
  status_code: 308  # Permanent redirect
```

**Block**: Return an error response
```yaml
action:
  type: block
  status_code: 410  # Gone
```

**Custom**: Return a custom response
```yaml
action:
  type: custom
  status_code: 403
  body: '{"error": "This API version is no longer available"}'
  content_type: application/json
```

### Replacement Info

```yaml
replacement:
  path: /api/v2/users
  preserve_query: true  # Preserve query string in redirects
  method: POST         # Optional: if the method changed
```

### Global Settings

```yaml
settings:
  deprecation_header: Deprecation      # Header name
  sunset_header: Sunset                # Header name
  link_header: Link                    # Header name for docs/replacement
  notice_header: X-Deprecation-Notice  # Header for human-readable message
  include_headers: true                # Add headers to responses
  past_sunset_action: warn             # warn, block, or redirect
  log_access: true                     # Log deprecated endpoint access
```

## Response Headers

When an endpoint is deprecated, the following headers are added:

```
Deprecation: @1704067200
Sunset: Sat, 01 Jun 2025 00:00:00 GMT
Link: <https://docs.example.com/migration>; rel="deprecation", </api/v2/users>; rel="successor-version"
X-Deprecation-Notice: This endpoint (/api/v1/users) is deprecated and will be removed on 2025-06-01. Please migrate to /api/v2/users.
```

## Metrics

The agent exposes Prometheus metrics for monitoring:

| Metric | Type | Description |
|--------|------|-------------|
| `zentinel_api_deprecation_requests_total` | counter | Total requests to deprecated endpoints |
| `zentinel_api_deprecation_redirects_total` | counter | Total redirects performed |
| `zentinel_api_deprecation_blocked_total` | counter | Total blocked requests |
| `zentinel_api_deprecation_days_until_sunset` | gauge | Days until endpoint sunset |
| `zentinel_api_deprecation_request_duration_seconds` | histogram | Request duration |

Enable metrics server:

```bash
zentinel-api-deprecation-agent --metrics --metrics-port 9090
```

## CLI Options

```
zentinel-api-deprecation-agent [OPTIONS]

Options:
  -c, --config <PATH>        Configuration file [default: api-deprecation.yaml]
  -s, --socket <PATH>        Unix socket path [default: /tmp/zentinel-api-deprecation.sock]
  -L, --log-level <LEVEL>    Log level [default: info]
      --print-config         Print default configuration
      --validate             Validate configuration and exit
      --metrics              Enable metrics server
      --metrics-port <PORT>  Metrics server port [default: 9090]
  -h, --help                 Print help
  -V, --version              Print version
```

## Use Cases

### Gradual API Migration

Track usage of v1 endpoints while migrating clients to v2:

```yaml
endpoints:
  - id: users-v1
    path: /api/v1/users
    status: deprecated
    sunset_at: "2025-06-01T00:00:00Z"
    replacement:
      path: /api/v2/users
    action:
      type: warn
```

### Immediate Redirect

Force clients to use the new endpoint:

```yaml
endpoints:
  - id: old-auth
    path: /auth/login
    status: deprecated
    replacement:
      path: /api/v2/auth/login
      preserve_query: true
    action:
      type: redirect
      status_code: 308
```

### Removed Endpoint

Return 410 Gone for completely removed endpoints:

```yaml
endpoints:
  - id: legacy-api
    path: /legacy/*
    status: removed
    documentation_url: https://docs.example.com/sunset-notice
    action:
      type: block
      status_code: 410
```

## License

Apache-2.0
