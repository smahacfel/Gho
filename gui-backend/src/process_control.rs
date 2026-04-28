//! Process control helpers for running Ghost launcher via tmux.

use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tokio::process::Command;

const DEFAULT_SESSION_NAME: &str = "ghost_launcher";

#[derive(Debug, Clone, Serialize)]
pub struct ProcessStatus {
    pub tmux_session_exists: bool,
    pub launcher_process_running: bool,
}

pub struct ProcessController {
    workspace_root: PathBuf,
    session_name: String,
}

impl Default for ProcessController {
    fn default() -> Self {
        // Przy pierwszym użyciu upewniamy się, że użytkownik root ma włączony
        // `linger` w systemd-logind — dzięki temu procesy użytkownika (w tym
        // serwer tmux) przeżywają rozłączenie SSH nawet gdy logind ma
        // KillUserProcesses=yes.  Błąd jest ignorowany (np. brak loginctl).
        let _ = std::process::Command::new("loginctl")
            .args(["enable-linger", "root"])
            .status();
        Self {
            workspace_root: detect_workspace_root(),
            session_name: DEFAULT_SESSION_NAME.to_string(),
        }
    }
}

impl ProcessController {
    pub fn new(workspace_root: PathBuf, session_name: String) -> Self {
        Self {
            workspace_root,
            session_name,
        }
    }

    pub async fn status(&self) -> Result<ProcessStatus> {
        Ok(ProcessStatus {
            tmux_session_exists: self.tmux_session_exists().await?,
            launcher_process_running: self.launcher_process_running().await?,
        })
    }

    pub async fn start_launcher(&self) -> Result<String> {
        self.ensure_tmux_installed().await?;

        if self.tmux_session_exists().await? {
            return Ok(format!(
                "Session '{}' already exists. Ghost launcher is likely running.",
                self.session_name
            ));
        }

        // Use tmux `-c` to avoid fragile shell quoting in a nested command.
        let root = shell_escape_string(&self.workspace_root.to_string_lossy());
        let session = shell_escape_string(&self.session_name);

        // `setsid` tworzy nową grupę procesów → serwer tmux jest odizolowany od
        // sesji SSH; `nohup` ignoruje SIGHUP na wypadek, gdyby sygnał dotarł mimo
        // setsid (np. przy KillUserProcesses=yes w systemd-logind).
        // `</dev/null` usuwa powiązanie ze stdin kontrolującego terminala.
        // remain-on-exit on:  okno tmux pozostaje otwarte po wyjściu launchera
        // (pozwala odczytać ostatni output po re-atach).
        let cmd = format!(
            "setsid nohup tmux new-session -d -s '{}' -c '{}' \
 env GHOST_GUI_BACKEND_DISABLED=1 cargo run --release --bin ghost-launcher \
 </dev/null >>/tmp/ghost_launcher_tmux.log 2>&1 \
 && tmux set-option -t '{}' remain-on-exit on 2>/dev/null || true",
            session, root, session
        );
        let output = shell(&cmd).await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            bail!("Failed to start launcher in tmux: {}", stderr);
        }

        Ok(format!(
            "Started ghost-launcher in tmux session '{}'.",
            self.session_name
        ))
    }

    pub async fn stop_launcher(&self) -> Result<String> {
        self.ensure_tmux_installed().await?;

        let session = shell_escape_string(&self.session_name);
        let kill_tmux_cmd = format!("tmux kill-session -t '{}'", session);
        let kill_output = shell(&kill_tmux_cmd).await?;
        if !kill_output.status.success() {
            let stderr = String::from_utf8_lossy(&kill_output.stderr)
                .trim()
                .to_string();
            // tmux returns non-zero when the session doesn't exist; treat that as already stopped.
            if !stderr.to_lowercase().contains("can't find session") {
                bail!("Failed to stop tmux session: {}", stderr);
            }
        }

        // Best-effort cleanup: killing the tmux session should be enough.
        // We avoid global `pkill ghost` (it can kill unrelated processes) and we also
        // intentionally do not fail the request if these best-effort cleanups don't match.
        let _ =
            shell("pkill -f \"cargo run --release --bin ghost-launcher\" >/dev/null 2>&1 || true")
                .await;
        let _ = shell("pkill -f \"target/(debug|release)/ghost-launcher\" >/dev/null 2>&1 || true")
            .await;

        Ok("Stop requested: launcher tmux session terminated.".to_string())
    }

    async fn tmux_session_exists(&self) -> Result<bool> {
        let session = shell_escape_string(&self.session_name);
        let cmd = format!("tmux has-session -t '{}' >/dev/null 2>&1", session);
        let output = shell(&cmd).await?;
        Ok(output.status.success())
    }

    async fn launcher_process_running(&self) -> Result<bool> {
        // Avoid patterns that can match the `pgrep` process itself.
        let a =
            shell("pgrep -f \"cargo run --release --bin ghost-launcher\" >/dev/null 2>&1").await?;
        if a.status.success() {
            return Ok(true);
        }
        let b = shell("pgrep -f \"target/(debug|release)/ghost-launcher\" >/dev/null 2>&1").await?;
        Ok(b.status.success())
    }

    async fn ensure_tmux_installed(&self) -> Result<()> {
        let output = shell("command -v tmux >/dev/null 2>&1").await?;
        if !output.status.success() {
            bail!("tmux is not installed or not available in PATH");
        }
        Ok(())
    }
}

fn detect_workspace_root() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| manifest_dir.to_path_buf())
}

fn shell_escape_string(input: &str) -> String {
    input.replace('\'', "'\"'\"'")
}

async fn shell(cmd: &str) -> Result<std::process::Output> {
    Command::new("bash")
        .arg("-lc")
        .arg(cmd)
        .output()
        .await
        .with_context(|| format!("Failed to execute shell command: {}", cmd))
}
