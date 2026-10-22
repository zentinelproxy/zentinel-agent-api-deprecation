//! Zentinel API Deprecation Agent - CLI Entry Point

use anyhow::Result;
use clap::Parser;
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;
use zentinel_agent_api_deprecation::{ApiDeprecationAgent, ApiDeprecationConfig};
use zentinel_agent_sdk::v2::{AgentRunnerV2, TransportConfig};

const DEFAULT_CONFIG: &str = "api-deprecation.yaml";
const DEFAULT_SOCKET: &str = "/tmp/zentinel-api-deprecation.sock";

#[derive(Parser, Debug)]
#[command(
    name = "zentinel-api-deprecation-agent",
    about = "API deprecation management agent for Zentinel proxy",
    version
)]
struct Args {
    /// Path to configuration file. Without it, `api-deprecation.yaml` in
    /// the working directory is used if it exists, otherwise the defaults
    /// (no deprecated endpoints).
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Unix socket path (UDS transport, agent protocol v2).
    /// Default: /tmp/zentinel-api-deprecation.sock. With --grpc-address the
    /// socket is served too only when given explicitly.
    #[arg(short, long)]
    socket: Option<PathBuf>,

    /// gRPC listen address (e.g. "127.0.0.1:50051")
    #[arg(long, value_name = "ADDR")]
    grpc_address: Option<SocketAddr>,

    /// Log level (trace, debug, info, warn, error)
    #[arg(short = 'L', long, default_value = "info")]
    log_level: Level,

    /// Print default configuration and exit
    #[arg(long)]
    print_config: bool,

    /// Validate configuration and exit
    #[arg(long)]
    validate: bool,

    /// Enable metrics server
    #[arg(long)]
    metrics: bool,

    /// Metrics server port
    #[arg(long, default_value = "9090")]
    metrics_port: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(args.log_level)
        .with_target(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    // Print default config if requested
    if args.print_config {
        let default_config = include_str!("../examples/default-config.yaml");
        println!("{}", default_config);
        return Ok(());
    }

    // Load configuration
    // An explicit --config must exist; the implicit api-deprecation.yaml
    // is optional.
    let config_path = args
        .config
        .clone()
        .or_else(|| Some(PathBuf::from(DEFAULT_CONFIG)).filter(|p| p.exists()));
    let config = if let Some(path) = config_path {
        if !path.exists() {
            anyhow::bail!("Configuration file not found: {}", path.display());
        }
        info!(path = ?path, "Loading configuration");
        ApiDeprecationConfig::from_file(&path)?
    } else if args.validate {
        anyhow::bail!("Configuration file not found: {}", DEFAULT_CONFIG);
    } else {
        info!("Using default configuration");
        ApiDeprecationConfig::default()
    };

    // Validate and exit if requested
    if args.validate {
        config.validate()?;
        println!("Configuration is valid");
        return Ok(());
    }

    // Create agent
    let agent = ApiDeprecationAgent::new(config);

    // Start metrics server if enabled
    if args.metrics {
        let metrics = agent.metrics().clone();
        let port = args.metrics_port;
        tokio::spawn(async move {
            start_metrics_server(metrics, port).await;
        });
    }

    // Determine transport configuration
    let transport = match (args.grpc_address, args.socket) {
        (Some(grpc_addr), Some(socket)) => {
            info!(
                grpc_address = %grpc_addr,
                socket = ?socket,
                "Starting API deprecation agent with gRPC and UDS transport (v2 protocol)"
            );
            TransportConfig::Both {
                grpc_address: grpc_addr,
                uds_path: socket,
            }
        }
        (Some(grpc_addr), None) => {
            info!(
                grpc_address = %grpc_addr,
                "Starting API deprecation agent with gRPC transport (v2 protocol)"
            );
            TransportConfig::Grpc { address: grpc_addr }
        }
        (None, socket) => {
            let socket = socket.unwrap_or_else(|| PathBuf::from(DEFAULT_SOCKET));
            info!(
                socket = ?socket,
                "Starting API deprecation agent with UDS transport (v2 protocol)"
            );
            TransportConfig::Uds { path: socket }
        }
    };

    // Run the agent with v2 protocol
    let mut runner = AgentRunnerV2::new(agent).with_name("api-deprecation");

    // Apply transport configuration
    runner = match transport {
        TransportConfig::Grpc { address } => runner.with_grpc(address),
        TransportConfig::Uds { path } => runner.with_uds(path),
        TransportConfig::Both {
            grpc_address,
            uds_path,
        } => runner.with_both(grpc_address, uds_path),
    };

    runner.run().await?;

    Ok(())
}

async fn start_metrics_server(
    metrics: zentinel_agent_api_deprecation::metrics::DeprecationMetrics,
    port: u16,
) {
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    let listener = match TcpListener::bind(format!("0.0.0.0:{}", port)).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(error = %e, "Failed to start metrics server");
            return;
        }
    };

    info!(port = port, "Metrics server started");

    loop {
        match listener.accept().await {
            Ok((mut socket, _)) => {
                let output = metrics.encode();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
                    output.len(),
                    output
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to accept metrics connection");
            }
        }
    }
}
