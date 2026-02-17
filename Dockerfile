# syntax=docker/dockerfile:1.4

# Zentinel API Deprecation Agent Container Image
#
# Targets:
#   - prebuilt: For CI with pre-built binaries

################################################################################
# Pre-built binary stage (for CI builds)
################################################################################
FROM gcr.io/distroless/cc-debian12:nonroot AS prebuilt

COPY zentinel-agent-api-deprecation /zentinel-agent-api-deprecation

LABEL org.opencontainers.image.title="Zentinel API Deprecation Agent" \
      org.opencontainers.image.description="Zentinel API Deprecation Agent for Zentinel reverse proxy" \
      org.opencontainers.image.vendor="Raskell" \
      org.opencontainers.image.source="https://github.com/zentinelproxy/zentinel-agent-api-deprecation"

ENV RUST_LOG=info,zentinel_agent_api_deprecation=debug \
    SOCKET_PATH=/var/run/zentinel/api-deprecation.sock

USER nonroot:nonroot

ENTRYPOINT ["/zentinel-agent-api-deprecation"]
