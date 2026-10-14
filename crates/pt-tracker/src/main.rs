//! pt-tracker 可执行程序。用法：`pt-tracker [配置文件]`（默认 `tracker.toml`；字段见 tracker.example.toml）

use std::net::SocketAddr;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let path = std::env::args().nth(1).unwrap_or_else(|| "tracker.toml".into());
    if path == "--version" || path == "-V" {
        println!("pt-tracker {}", env!("CARGO_PKG_VERSION"));
        return std::process::ExitCode::SUCCESS;
    }
    let cfg: pt_tracker::Config = match std::fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|t| toml::from_str(&t).map_err(|e| e.to_string()))
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {path}: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    let bind = cfg.bind.clone();
    let router = match pt_tracker::start(cfg).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return std::process::ExitCode::from(3);
        }
    };
    let listener = match tokio::net::TcpListener::bind(&bind).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: bind {bind}: {e}");
            return std::process::ExitCode::from(4);
        }
    };
    tracing::info!(%bind, "pt-tracker listening");
    let server = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    });
    if let Err(e) = server.await {
        tracing::error!(error = %e, "server error");
    }
    std::process::ExitCode::SUCCESS
}
