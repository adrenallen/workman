use std::{env, error::Error, path::PathBuf};

use gbuildd::{DaemonConfig, DaemonServer};
use tokio::signal::unix::{SignalKind, signal};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = parse_args()?;
    let server = DaemonServer::bind(config).await?;
    let discovery = server.discovery().clone();

    println!(
        "{} daemon listening on 127.0.0.1:{}",
        gbuild_core::PROJECT_NAME,
        discovery.port
    );

    server.serve_until(shutdown_signal()).await?;
    Ok(())
}

fn parse_args() -> Result<DaemonConfig, Box<dyn Error>> {
    let mut config = DaemonConfig::default();
    let mut args = env::args_os().skip(1);

    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--data-dir") => {
                let value = args.next().ok_or("--data-dir requires a path")?;
                config.data_dir = PathBuf::from(value);
            }
            Some("--port") => {
                let value = args.next().ok_or("--port requires a number")?;
                let value = value.to_str().ok_or("--port must be valid UTF-8")?;
                config.port = value.parse()?;
            }
            Some("--help" | "-h") => {
                println!("Usage: gbuildd [--data-dir PATH] [--port PORT]");
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument: {}", arg.to_string_lossy()).into()),
        }
    }

    Ok(config)
}

async fn shutdown_signal() {
    let mut terminate = signal(SignalKind::terminate()).ok();

    if let Some(terminate) = terminate.as_mut() {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    } else {
        let _ = tokio::signal::ctrl_c().await;
    }
}
