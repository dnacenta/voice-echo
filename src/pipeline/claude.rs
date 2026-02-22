use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::sync::Mutex;

/// Bridge to Claude Code CLI. Manages conversation sessions per call.
pub struct ClaudeBridge {
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    session_timeout: Duration,
    dangerously_skip_permissions: bool,
    soul_path: Option<std::path::PathBuf>,
}

struct Session {
    conversation_id: Option<String>,
    last_used: Instant,
}

impl ClaudeBridge {
    pub fn new(
        session_timeout_secs: u64,
        dangerously_skip_permissions: bool,
        soul_path: Option<std::path::PathBuf>,
    ) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            session_timeout: Duration::from_secs(session_timeout_secs),
            dangerously_skip_permissions,
            soul_path,
        }
    }

    /// Send a prompt to Claude Code CLI and get the response text.
    ///
    /// Uses conversation continuation (`-r`) if a previous conversation
    /// exists for this call session, enabling multi-turn voice chats.
    pub async fn send(&self, call_sid: &str, prompt: &str) -> Result<String, ClaudeError> {
        let mut sessions = self.sessions.lock().await;

        // Clean up expired sessions
        sessions.retain(|_, s| s.last_used.elapsed() < self.session_timeout);

        let session = sessions
            .entry(call_sid.to_string())
            .or_insert_with(|| Session {
                conversation_id: None,
                last_used: Instant::now(),
            });

        let mut cmd = Command::new("claude");
        cmd.arg("-p").arg(prompt).arg("--output-format").arg("json");

        if self.dangerously_skip_permissions {
            cmd.arg("--dangerously-skip-permissions");
        }

        // Inject soul document if configured
        if let Some(ref path) = self.soul_path {
            if let Ok(contents) = std::fs::read_to_string(path) {
                cmd.arg("--append-system-prompt").arg(contents);
            }
        }

        // Continue existing conversation if we have one
        if let Some(ref conv_id) = session.conversation_id {
            cmd.arg("-r").arg(conv_id);
        }

        session.last_used = Instant::now();
        drop(sessions); // Release lock during CLI execution

        tracing::info!(call_sid, "Sending prompt to Claude CLI");

        let output = cmd
            .output()
            .await
            .map_err(|e| ClaudeError::Spawn(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ClaudeError::Cli(format!(
                "Exit {}: {}",
                output.status,
                stderr.trim()
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let parsed: ClaudeJsonOutput = serde_json::from_str(&stdout)
            .map_err(|e| ClaudeError::Cli(format!("Failed to parse JSON: {e}")))?;

        // Store session ID for conversation continuity across turns
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get_mut(call_sid) {
            session.conversation_id = Some(parsed.session_id);
            session.last_used = Instant::now();
        }

        tracing::info!(
            call_sid,
            response_len = parsed.result.len(),
            "Claude responded"
        );

        Ok(parsed.result)
    }

    /// Remove a session (call ended).
    pub async fn end_session(&self, call_sid: &str) {
        self.sessions.lock().await.remove(call_sid);
    }
}

#[derive(serde::Deserialize)]
struct ClaudeJsonOutput {
    result: String,
    session_id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ClaudeError {
    #[error("Failed to spawn claude CLI: {0}")]
    Spawn(String),
    #[error("Claude CLI error: {0}")]
    Cli(String),
}
