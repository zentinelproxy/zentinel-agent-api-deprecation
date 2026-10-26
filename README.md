<p align="center">
  <a href="https://zentinelproxy.io">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/zentinelproxy/design/main/marks/lockup/zentinel-lockup-dark.svg">
      <img alt="Zentinel" src="https://raw.githubusercontent.com/zentinelproxy/design/main/marks/lockup/zentinel-lockup-light.svg" height="52">
    </picture>
  </a>
</p>

<h3 align="center">API deprecation agent</h3>

<p align="center">Marks, redirects or retires deprecated API endpoints, decided on the request headers before the request reaches the upstream.</p>

<p align="center">
  <a href="https://github.com/zentinelproxy/zentinel-agent-api-deprecation/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/zentinelproxy/zentinel-agent-api-deprecation/actions/workflows/ci.yml/badge.svg"></a>
  <img alt="License: Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-9b4d8a?style=flat&amp;labelColor=1f161e">
  <img alt="Agent protocol v2" src="https://img.shields.io/badge/agent%20protocol-v2-f68d31?style=flat&amp;labelColor=1f161e">
</p>

<p align="center">
  <a href="https://registry.zentinelproxy.io/api-deprecation/">Registry</a> &middot;
  <a href="https://docs.zentinelproxy.io">Docs</a> &middot;
  <a href="https://github.com/zentinelproxy">All repositories</a>
</p>

For teams retiring old API versions behind Zentinel. You list deprecated
endpoints in a YAML file (path or glob, methods, status, sunset date,
replacement); the agent matches each request against that list and either lets
it through with deprecation headers, redirects it to the replacement, or
answers it itself (`410 Gone` or a custom response). Requests that match no
entry are allowed untouched. If the agent is unreachable, the filter's
`failure-mode` decides: with `"open"` the request goes to the upstream as if
nothing were deprecated.

## Quick start

```bash
zentinel bundle install api-deprecation
```

Write `api-deprecation.yaml`:

```yaml
endpoints:
  - id: legacy-users-api
    path: /api/v1/users
    methods: [GET, POST]
    status: deprecated
    sunset_at: "2027-06-01T00:00:00Z"
    replacement:
      path: /api/v2/users
    documentation_url: https://docs.example.com/migration
    action:
      type: block
      status_code: 410
```

Start the agent:

```bash
zentinel-api-deprecation-agent --config api-deprecation.yaml --socket /tmp/zentinel-api-deprecation.sock
```

Wire it into a route (this file passes `zentinel test -c`):

```kdl
system {
    worker-threads 0
}

listeners {
    listener "http" {
        address "0.0.0.0:8080"
        protocol "http"
    }
}

upstreams {
    upstream "backend" {
        target "127.0.0.1:3000"
    }
}

agents {
    agent "api-deprecation" type="custom" {
        unix-socket "/tmp/zentinel-api-deprecation.sock"
        events "request_headers"
        timeout-ms 100
    }
}

filters {
    filter "api-deprecation" {
        type "agent"
        agent "api-deprecation"
        failure-mode "open"
    }
}

routes {
    route "api" {
        matches { path-prefix "/" }
        upstream "backend"
        filters "api-deprecation"
    }
}
```

`failure-mode "open"` keeps the API available when the agent is down; use
`"closed"` if serving a removed endpoint is worse than refusing traffic.

## Install

### Bundle (recommended)

```bash
zentinel bundle install api-deprecation
```

Downloads the release binary pinned in Zentinel's bundle lock and installs it as `zentinel-api-deprecation-agent`.

### Release binaries

Tarballs for `linux-x86_64`, `linux-aarch64` and `darwin-aarch64` are attached to each [release](https://github.com/zentinelproxy/zentinel-agent-api-deprecation/releases).

### From source

`cargo install --git` does not work for this crate: it depends on a sibling
checkout of [`zentinel-agent-rust-sdk`](https://github.com/zentinelproxy/zentinel-agent-rust-sdk) by path.

```bash
git clone https://github.com/zentinelproxy/zentinel-agent-rust-sdk
git clone https://github.com/zentinelproxy/zentinel-agent-api-deprecation
cd zentinel-agent-api-deprecation
cargo build --release   # target/release/zentinel-api-deprecation-agent
```

## Configuration

The agent reads a YAML file and its command line. Zentinel 26.10_8 and later
send the agent block's `config { }` on every connection (releases up to 0.6.50
never did), but the agent has no handler for it: it acknowledges the block
and applies nothing, so set everything here.

| Option | Default | Meaning |
|---|---|---|
| `-c`, `--config` | `api-deprecation.yaml` if present | YAML file to load. Without the option, `api-deprecation.yaml` in the working directory is used if it exists, otherwise the agent starts with no endpoints (allows everything). A `--config` file that does not exist is an error. |
| `-s`, `--socket` | `/tmp/zentinel-api-deprecation.sock` | Unix socket to listen on (agent protocol v2). |
| `--grpc-address` | none | Listen for gRPC (agent protocol v2) on this address instead, e.g. `127.0.0.1:50051`. The Unix socket is served as well only if `--socket` is also given. |
| `-L`, `--log-level` | `info` | `trace`, `debug`, `info`, `warn` or `error`. |
| `--print-config` | | Print the annotated example configuration and exit. |
| `--validate` | | Parse and validate the file, then exit. |
| `--metrics` | off | Serve Prometheus metrics over HTTP on `0.0.0.0:<metrics-port>`. |
| `--metrics-port` | `9090` | Port for `--metrics`. |
| `--version` | | Print `zentinel-api-deprecation-agent <version>` and exit. |

Unknown keys in the file are rejected, so a typo fails at startup rather than
being ignored.

### Endpoint entries (`endpoints[]`)

The first entry that matches wins.

| Option | Default | Meaning |
|---|---|---|
| `id` | required | Name used in logs and metric labels. |
| `path` | required | Without `*` or `?`: the exact path and everything under `path/`. With them: a glob, e.g. `/api/v1/*`. |
| `methods` | `[]` (all) | Methods to match, case-insensitive. |
| `status` | `deprecated` | `deprecated`, `scheduled` or `removed`. `removed` always answers `410 Gone`, whatever `action` says. |
| `deprecated_at` | none | RFC 3339 time; sets `Deprecation: @<unix-seconds>` (otherwise `Deprecation: true`). |
| `sunset_at` | none | RFC 3339 time; sets the `Sunset` header (RFC 8594). After it passes, `settings.past_sunset_action` replaces `action`. |
| `replacement.path` | none | Successor path: `Link: <...>; rel="successor-version"` and the redirect target. Required by `action.type: redirect`. |
| `replacement.preserve_query` | `true` | Append the request's query string to the redirect target. |
| `documentation_url` | none | `Link: <...>; rel="deprecation"`, quoted in the notice header and returned as `documentation` in the error body. |
| `message` | generated | Text of the notice header. |
| `action.type` | `warn` | `warn`, `redirect`, `block` or `custom`, see below. |
| `action.status_code` | `308` (redirect), `410` (block) | Status for `redirect`, `block` and `custom` (required for `custom`). |
| `action.body`, `action.content_type` | none, `application/json` | Response for `custom` (`body` required). |
| `headers` | `{}` | Extra headers added alongside the deprecation headers. |
| `track_usage` | `true` | Count requests to this entry in the metrics. |

`replacement.method` and `replacement.param_mappings` are parsed but not used.

Actions:

- `warn`: allow the request and ask for the deprecation headers on the
  response. **Zentinel 26.10_8 and later send them to the client; releases up
  to 26.10_7 (0.6.50) dropped response headers returned on a request-headers
  decision, so there `warn` only logs and counts.**
- `redirect`: answer with `status_code` and `Location: <replacement>`, plus the
  deprecation headers. Any 3xx works (sent as a block response with a
  `Location` header; until 0.5.0, 301 and 302 used the protocol's redirect
  decision, which Zentinel releases up to 26.10_7 answered with a 500 and
  26.10_8 redirects with the agent's `Location`).
- `block`: answer with `status_code`, the deprecation headers and a JSON body
  (`"error": "endpoint_removed"`, a `message` naming the path, and `documentation` if set).
- `custom`: answer with `status_code`, `body` and `content_type`, without
  deprecation headers.

### Global settings (`settings`)

| Option | Default | Meaning |
|---|---|---|
| `deprecation_header` | `Deprecation` | Name of the deprecation header. |
| `sunset_header` | `Sunset` | Name of the sunset header. |
| `link_header` | `Link` | Name of the link header. |
| `notice_header` | `X-Deprecation-Notice` | Name of the human-readable notice header. |
| `past_sunset_action` | `warn` | For entries past `sunset_at`: `warn`, `block` (410) or `redirect` (301, falls back to 410 without a replacement). |
| `log_access` | `true` | Log every matched request at `info`. |
| `include_headers` | `true` | Parsed but not used: headers are always built. |

### Metrics (`metrics`)

| Option | Default | Meaning |
|---|---|---|
| `prefix` | `zentinel_api_deprecation` | Prefix for the Prometheus metric names. |
| `enabled`, `port`, `labels` | `true`, `0`, `{}` | Parsed but not used; the metrics server is started by `--metrics`. |

With `--metrics`, the agent serves `<prefix>_requests_total`,
`<prefix>_redirects_total`, `<prefix>_blocked_total` (counters) and
`<prefix>_days_until_sunset` (gauge). `<prefix>_request_duration_seconds` is
registered but never observed. It also reports the endpoint count and days
until sunset to Zentinel over the protocol.

Full reference: <https://registry.zentinelproxy.io/api-deprecation/>

## How it works

The agent handles only the request-headers event. It matches the request path
and method against `endpoints` in order, records the match, picks the action
(`removed` status first, then the past-sunset policy, then the entry's
`action`) and either allows the request or returns the response itself. Path
globs use [`globset`](https://docs.rs/globset); without a glob character a path
also matches everything below it (`/api/v1/users` matches `/api/v1/users/42`).
Only the path is matched: a query string does not change the result (before
0.5.0 it did, so `/removed?x=1` got past a `removed` entry).

## Releases and smoke test

Releases are built from their tag by the shared pipeline in
[`zentinelproxy/.github`](https://github.com/zentinelproxy/.github)
(`agent-release.yml`): each platform's binary is run behind a real Zentinel
by [`scripts/smoke-test.sh`](scripts/smoke-test.sh) before anything is
published, and CI runs the same smoke job on every push. The smoke test uses
[`scripts/smoke/api-deprecation.yaml`](scripts/smoke/api-deprecation.yaml)
over the Unix socket and over gRPC, fail-closed: a path that is not deprecated
and a deprecated one with a future sunset are forwarded, a `removed` entry and
one past its sunset get `410`, a redirect entry gets `301`, and with the agent
stopped requests are refused. Locally:

```bash
AGENT_SMOKE=../.github/agent-smoke/smoke.py \
  scripts/smoke-test.sh target/release/zentinel-api-deprecation-agent ./zentinel
```

## Development

```bash
git clone https://github.com/zentinelproxy/zentinel-agent-rust-sdk ../zentinel-agent-rust-sdk
cargo build
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

Minimum toolchain: stable Rust (CI uses `dtolnay/rust-toolchain@stable`) and
`protoc` (CI installs `protobuf-compiler`).

## Related

- [Registry page](https://registry.zentinelproxy.io/api-deprecation/)
- [Agent configuration](https://docs.zentinelproxy.io/configuration/agents/)
- [Agent protocol v2](https://docs.zentinelproxy.io/agents/v2/)
- [Zentinel](https://github.com/zentinelproxy/zentinel)
- [RFC 8594: The Sunset HTTP header](https://www.rfc-editor.org/rfc/rfc8594)

## License

Apache-2.0, see [`LICENSE`](LICENSE).

---

<p align="center">
  <sub>Part of <a href="https://zentinelproxy.io">Zentinel</a>, the security-first reverse proxy. The shisa guards the gate.</sub>
</p>
