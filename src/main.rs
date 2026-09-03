//! Totem Runtime Studio (Sentinel Runtime) — main binary.
//!
//! Starts two servers concurrently:
//!   - HTTP  (Axum)  on --bind (default 127.0.0.1:3030)
//!   - gRPC  (Tonic) on --grpc-bind (default 127.0.0.1:3032)

use anyhow::Result;
use api::RuntimeContext;
use clap::Parser;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(
    name = "sentinel",
    about = "Totem Runtime Studio (Sentinel Runtime) - Secure execution runtime for AI agent workflows",
    version
)]
struct Args {
    /// HTTP server bind address
    #[arg(short, long, default_value = "127.0.0.1:3030")]
    bind: String,

    /// gRPC server bind address
    #[arg(long, default_value = "127.0.0.1:3032")]
    grpc_bind: String,

    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long, default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| {
                    format!("totem_runtime_studio={},tower_http=debug", args.log_level).into()
                }),
        )
        .init();

    eprintln!("   Totem Runtime Studio (Sentinel Runtime) starting...");
    eprintln!("   Version : {}", env!("CARGO_PKG_VERSION"));
    eprintln!("   HTTP    : http://{}", args.bind);
    eprintln!("   gRPC    : {}", args.grpc_bind);
    tracing::info!(
        "🚀 Totem Runtime Studio v{} — HTTP:{} gRPC:{}",
        env!("CARGO_PKG_VERSION"),
        args.bind,
        args.grpc_bind
    );

    // Build runtime context — all subsystems wired here, shared across HTTP + gRPC
    let ctx = Arc::new(RuntimeContext::with_bind(Some(args.bind.clone())));

    let http_app  = api::http::app(Arc::clone(&ctx));
    let http_listener = tokio::net::TcpListener::bind(&args.bind).await?;

    eprintln!("✓ Ready. HTTP endpoints:");
    eprintln!("   POST http://{}/execute                   — execute task workflow", args.bind);
    eprintln!("   GET  http://{}/audit                     — hash-chained audit trail", args.bind);
    eprintln!("   GET  http://{}/health                    — health + version", args.bind);
    eprintln!("   GET  http://{}/tools                     — registered tool list", args.bind);
    eprintln!("   GET  http://{}/.well-known/agent.json    — A2A Agent Card", args.bind);
    eprintln!("✓ gRPC endpoints:");
    eprintln!("   /sentinel.Runtime/ExecuteTask            — execute task");
    eprintln!("   /sentinel.Runtime/GetAuditLog            — audit log");

    // Run HTTP and gRPC concurrently — both use the same RuntimeContext
    let grpc_addr  = args.grpc_bind.clone();
    let grpc_ctx   = Arc::clone(&ctx);

    tokio::select! {
        result = axum::serve(http_listener, http_app) => {
            result?;
        }
        result = api::grpc::start_grpc_server(grpc_ctx, &grpc_addr) => {
            result.map_err(|e| anyhow::anyhow!("gRPC server error: {e}"))?;
        }
    }

    Ok(())
}
