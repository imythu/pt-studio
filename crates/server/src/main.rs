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
    /// 插件开发工具（不读取 site.toml、不连接数据库）
    Plugin {
        #[command(subcommand)]
        cmd: PluginCmd,
    },
}

#[derive(Debug, Subcommand)]
enum PluginCmd {
    /// 检查插件包目录：清单、API 版本、组件链接与导出、迁移文件名
    Check {
        /// 包目录（`<id>/` 下含 plugin.toml、plugin.wasm）
        dir: std::path::PathBuf,
    },
    /// 组装插件包：<out>/<id>/{plugin.toml, plugin.wasm, migrations/, checksums.txt}，并检查
    Package {
        /// 插件源码目录（含 plugin.toml、migrations/）
        #[arg(long, default_value = ".")]
        src: std::path::PathBuf,
        /// 构建出的组件（cargo build --release --target wasm32-wasip2 的产物）
        #[arg(long)]
        wasm: std::path::PathBuf,
        /// 输出目录（站点配置 plugins.dir 指向它即可加载）
        #[arg(long, default_value = "dist")]
        out: std::path::PathBuf,
    },
}

fn plugin_cmd(cmd: PluginCmd) -> ExitCode {
    let r = match cmd {
        PluginCmd::Check { dir } => {
            pt_server::check(&dir).map(|m| format!("ok: {} {} ({})", m.id, m.version, m.api))
        }
        PluginCmd::Package { src, wasm, out } => {
            pt_server::package(&src, &wasm, &out).map(|d| format!("ok: packaged {}", d.display()))
        }
    };
    match r {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
    let cli = Cli::parse();
    if let Cmd::Plugin { cmd } = cli.cmd {
        return plugin_cmd(cmd);
    }
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
        Cmd::Plugin { .. } => unreachable!("handled above"),
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
