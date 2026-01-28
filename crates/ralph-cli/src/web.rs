//! # ralph web
//!
//! Web dashboard development server launcher.
//!
//! This module provides the `ralph web` command that runs both the backend
//! and frontend dev servers in parallel.

use anyhow::{Context, Result};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tokio::process::{Child, Command as AsyncCommand};

#[cfg(unix)]
use nix::sys::signal::{Signal, kill};
#[cfg(unix)]
use nix::unistd::Pid;

/// Grace period for servers to shut down before SIGKILL (matches backend's SIGINT handler)
const SHUTDOWN_GRACE_PERIOD: Duration = Duration::from_secs(10);

/// Arguments for the web subcommand
#[derive(Parser, Debug)]
pub struct WebArgs {
    /// Backend port (default: 3000)
    #[arg(long, default_value = "3000")]
    pub backend_port: u16,

    /// Frontend port (default: 5173)
    #[arg(long, default_value = "5173")]
    pub frontend_port: u16,

    /// Workspace root directory (default: current directory)
    #[arg(long)]
    pub workspace: Option<PathBuf>,
}

/// Check that Node.js is installed and >= 18. Returns the version string.
fn check_node() -> Result<String> {
    let output = Command::new("node")
        .arg("--version")
        .output()
        .map_err(|_| {
            anyhow::anyhow!(
                "Node.js is not installed or not in PATH.\n\
                 Install Node.js 18+: https://nodejs.org/\n\
                 Or via nvm: nvm install 18"
            )
        })?;

    if !output.status.success() {
        anyhow::bail!(
            "Failed to run `node --version`.\n\
             Install Node.js 18+: https://nodejs.org/"
        );
    }

    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // Parse major version from e.g. "v18.17.0"
    let major: u32 = version
        .trim_start_matches('v')
        .split('.')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    if major < 18 {
        anyhow::bail!(
            "Node.js {} is too old (need >= 18).\n\
             Update: https://nodejs.org/ or `nvm install 18`",
            version
        );
    }

    Ok(version)
}

/// Check that npm is installed and working. Returns the version string.
fn check_npm() -> Result<String> {
    let output = Command::new("npm").arg("--version").output().map_err(|_| {
        anyhow::anyhow!(
            "npm is not installed or not in PATH.\n\
             npm should come with Node.js. Try reinstalling Node: https://nodejs.org/"
        )
    })?;

    if !output.status.success() {
        anyhow::bail!(
            "Failed to run `npm --version`.\n\
             Try reinstalling Node.js: https://nodejs.org/"
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Check if npm dependencies need to be installed.
fn needs_install(root: &Path) -> bool {
    !root.join("node_modules/.package-lock.json").exists()
}

/// Run npm install (or npm ci if lockfile present) with a spinner.
async fn run_npm_install(root: &Path) -> Result<()> {
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .expect("valid template"),
    );

    let has_lockfile = root.join("package-lock.json").exists();
    let install_cmd = if has_lockfile { "ci" } else { "install" };

    spinner.set_message(format!("Running npm {}...", install_cmd));
    spinner.enable_steady_tick(Duration::from_millis(100));

    let output = AsyncCommand::new("npm")
        .arg(install_cmd)
        .current_dir(root)
        .output()
        .await
        .context("Failed to run npm install")?;

    spinner.finish_and_clear();

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("npm {} failed:\n{}", install_cmd, stderr.trim());
    }

    println!("Dependencies installed successfully.");
    Ok(())
}

/// Run pre-flight checks: verify Node.js/npm and auto-install dependencies.
async fn preflight(root: &Path) -> Result<()> {
    let node_version = check_node()?;
    let npm_version = check_npm()?;
    println!(
        "Using Node {} with npm {}",
        node_version.trim_start_matches('v'),
        npm_version
    );

    if needs_install(root) {
        println!("node_modules not found — installing dependencies...");
        run_npm_install(root).await?;
    }

    Ok(())
}

/// Run both backend and frontend dev servers in parallel
pub async fn execute(args: WebArgs) -> Result<()> {
    println!("🌐 Starting Ralph web servers...");
    println!(
        "   Backend: http://localhost:{}\n   Frontend: http://localhost:{}",
        args.backend_port, args.frontend_port
    );
    println!();

    // Determine workspace root: explicit flag or current directory
    let workspace_root = match args.workspace {
        Some(path) => {
            // Canonicalize to get absolute path
            path.canonicalize()
                .with_context(|| format!("Invalid workspace path: {}", path.display()))?
        }
        None => env::current_dir().context("Failed to get current directory")?,
    };

    // Verify Node.js/npm and auto-install dependencies if needed
    preflight(&workspace_root).await?;

    println!("Using workspace: {}", workspace_root.display());

    // Compute absolute paths for backend and frontend directories
    // This ensures they work correctly regardless of where `ralph web` is invoked from
    let backend_dir = workspace_root.join("backend/ralph-web-server");
    let frontend_dir = workspace_root.join("frontend/ralph-web");

    // Spawn backend server
    // Pass RALPH_WORKSPACE_ROOT so the backend knows where to spawn ralph run from
    let mut backend = AsyncCommand::new("npm")
        .args(["run", "dev"])
        .current_dir(&backend_dir)
        .env("RALPH_WORKSPACE_ROOT", &workspace_root)
        .spawn()
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to start backend server. Is npm installed and {} set up?\nError: {}",
                backend_dir.join("package.json").display(),
                e
            )
        })?;

    // Spawn frontend server
    let mut frontend = AsyncCommand::new("npm")
        .args(["run", "dev"])
        .current_dir(&frontend_dir)
        .spawn()
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to start frontend server. Is npm installed and {} set up?\nError: {}",
                frontend_dir.join("package.json").display(),
                e
            )
        })?;

    println!("Press Ctrl+C to stop both servers");

    // Set up shutdown channel
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

    // Spawn signal handlers
    let shutdown_tx_sigint = shutdown_tx.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            println!("\nReceived Ctrl+C, shutting down servers...");
            let _ = shutdown_tx_sigint.send(true);
        }
    });

    #[cfg(unix)]
    {
        let shutdown_tx_sigterm = shutdown_tx.clone();
        tokio::spawn(async move {
            let mut sigterm =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("Failed to register SIGTERM handler");
            sigterm.recv().await;
            println!("\nReceived SIGTERM, shutting down servers...");
            let _ = shutdown_tx_sigterm.send(true);
        });

        let shutdown_tx_sighup = shutdown_tx.clone();
        tokio::spawn(async move {
            let mut sighup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())
                .expect("Failed to register SIGHUP handler");
            sighup.recv().await;
            println!("\nReceived SIGHUP (terminal closed), shutting down servers...");
            let _ = shutdown_tx_sighup.send(true);
        });
    }

    // Wait for shutdown signal or server exit
    tokio::select! {
        _ = shutdown_rx.changed() => {
            // Signal received - gracefully terminate both servers
            println!("Stopping backend server...");
            terminate_gracefully(&mut backend, SHUTDOWN_GRACE_PERIOD).await;
            println!("Stopping frontend server...");
            terminate_gracefully(&mut frontend, SHUTDOWN_GRACE_PERIOD).await;
            println!("All servers stopped.");
        }
        r = backend.wait() => {
            println!("Backend exited: {:?}", r);
            // Gracefully terminate frontend on backend exit
            println!("Stopping frontend server...");
            terminate_gracefully(&mut frontend, SHUTDOWN_GRACE_PERIOD).await;
        }
        r = frontend.wait() => {
            println!("Frontend exited: {:?}", r);
            // Gracefully terminate backend on frontend exit
            println!("Stopping backend server...");
            terminate_gracefully(&mut backend, SHUTDOWN_GRACE_PERIOD).await;
        }
    }

    Ok(())
}

/// Gracefully terminate a child process by sending SIGTERM first, then SIGKILL after grace period
#[cfg(unix)]
async fn terminate_gracefully(child: &mut Child, grace_period: Duration) {
    if let Some(pid) = child.id() {
        let pid = Pid::from_raw(pid as i32);

        // Send SIGTERM for graceful shutdown
        if kill(pid, Signal::SIGTERM).is_err() {
            // Process may have already exited
            let _ = child.wait().await;
            return;
        }

        // Wait for graceful exit with timeout
        match tokio::time::timeout(grace_period, child.wait()).await {
            Ok(_) => {
                // Process exited gracefully
            }
            Err(_) => {
                // Grace period elapsed, force kill
                println!("  Grace period elapsed, forcing termination...");
                let _ = kill(pid, Signal::SIGKILL);
                let _ = child.wait().await;
            }
        }
    } else {
        // No PID means process already exited or wasn't started
        let _ = child.wait().await;
    }
}

/// Gracefully terminate a child process (non-Unix fallback using start_kill)
#[cfg(not(unix))]
async fn terminate_gracefully(child: &mut Child, _grace_period: Duration) {
    let _ = child.start_kill();
    let _ = child.wait().await;
}
