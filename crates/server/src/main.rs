//! pt-server CLI（kernel/03 §17）：`serve`、`check`、`migrate`。

use std::{net::SocketAddr, process::ExitCode};

use clap::{Parser, Subcommand};
use pt_kernel::{Kernel, config::SiteConfig};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "pt-server", version, about = "PT Studio server")]
struct Cli {
    /// site.toml 路径
    #[arg(long, short, env = "PT_CONFIG", default_value = "site.toml")]
    config: String,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// 启动服务
    Serve,
    /// 清单与依赖检查（不连接数据库）
    Check,
    /// 只执行迁移
    Migrate,
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
    let cli = Cli::parse();
    let cfg = match std::fs::read_to_string(&cli.config)
        .map_err(anyhow::Error::from)
        .and_then(|t| SiteConfig::parse(&t))
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error[E-CONFIG] {}: {e}", cli.config);
            return ExitCode::from(2);
        }
    };
    match cli.cmd {
        Cmd::Check => match pt_kernel::kernel::check(&cfg, pt_server::native_plugins()) {
            Ok(res) => {
                println!("ok: activation order {}", res.order.join(" → "));
                ExitCode::SUCCESS
            }
            Err(errs) => {
                for e in errs {
                    eprintln!("{e}");
                }
                ExitCode::from(2)
            }
        },
        Cmd::Migrate => {
            let mut cfg = cfg;
            cfg.database.migrate_on_start = true;
            match pt_kernel::kernel::migrate_only(&cfg, pt_server::native_plugins()).await {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("{e}");
                    ExitCode::from(u8::try_from(e.exit_code()).unwrap_or(1))
                }
            }
        }
        Cmd::Serve => {
            let bind = cfg.http.bind.clone();
            let kernel = match Kernel::start(cfg, pt_server::native_plugins()).await {
                Ok(k) => k,
                Err(e) => {
                    eprintln!("{e}");
                    return ExitCode::from(u8::try_from(e.exit_code()).unwrap_or(1));
                }
            };
            let app = kernel.router();
            let listener = match tokio::net::TcpListener::bind(&bind).await {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("error: bind {bind}: {e}");
                    return ExitCode::from(4);
                }
            };
            tracing::info!(%bind, "listening");
            let server = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
                .with_graceful_shutdown(async {
                    let _ = tokio::signal::ctrl_c().await;
                });
            if let Err(e) = server.await {
                tracing::error!(error = %e, "server error");
            }
            kernel.shutdown().await;
            ExitCode::SUCCESS
        }
    }
}
