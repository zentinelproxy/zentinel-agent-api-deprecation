//! Zentinel API Deprecation Agent - CLI Entry Point

use anyhow::Result;
use clap::Parser;
use zentinel_agent_api_deprecation::{ApiDeprecationAgent, ApiDeprecationConfig};
use zentinel_agent_sdk::v2::{AgentRunnerV2, TransportConfig};
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[derive(Parser, Debug)]
#[command(
    name = "zentinel-agent-api-deprecation",
    about = "API deprecation management agent for Zentinel proxy",
    version
)]
struct Args {
    /// Path to configuration file
    #[arg(short, long, default_value = "api-deprecation.yaml")]
    config: PathBuf,

    /// Unix socket path for agent communication
    #[arg(short, long, default_value = "/tmp/zentinel-api-deprecation.sock")]
    socket: PathBuf,

    /// gRPC server address (e.g., "0.0.0.0:50051")
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
    let config = if args.config.exists() {
        info!(path = ?args.config, "Loading configuration");
        ApiDeprecationConfig::from_file(&args.config)?
    } else if args.validate {
        anyhow::bail!("Configuration file not found: {:?}", args.config);
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
    let transport = match args.grpc_address {
        Some(grpc_addr) => {
            info!(
                grpc_address = %grpc_addr,
                socket = ?args.socket,
                "Starting API deprecation agent with gRPC and UDS transport (v2 protocol)"
            );
            TransportConfig::Both {
                grpc_address: grpc_addr,
                uds_path: args.socket,
            }
        }
        None => {
            info!(
                socket = ?args.socket,
                "Starting API deprecation agent with UDS transport (v2 protocol)"
            );
            TransportConfig::Uds {
                path: args.socket,
            }
        }
    };

    // Run the agent with v2 protocol
    let mut runner = AgentRunnerV2::new(agent)
        .with_name("api-deprecation");

    // Apply transport configuration
    runner = match transport {
        TransportConfig::Grpc { address } => runner.with_grpc(address),
        TransportConfig::Uds { path } => runner.with_uds(path),
        TransportConfig::Both { grpc_address, uds_path } => {
            runner.with_both(grpc_address, uds_path)
        }
    };

    runner.run().await?;

    Ok(())
}

async fn start_metrics_server(metrics: zentinel_agent_api_deprecation::metrics::DeprecationMetrics, port: u16) {
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
