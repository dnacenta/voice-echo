use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use echo_system_types::llm::{LmProvider, Message, MessageContent, Role};
use tokio::sync::Mutex;

/// LLM conversation manager. Maintains per-call message history and invokes
/// the provider with the full history on each turn.
///
/// Drop-in replacement for ClaudeBridge — same `send()` / `end_session()` API,
/// but backed by any `Arc<dyn LmProvider>` instead of the Claude CLI subprocess.
pub struct ConversationManager {
    provider: Arc<dyn LmProvider>,
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    session_timeout: Duration,
    system_prompt: String,
    max_response_tokens: u32,
}

struct Session {
    messages: Vec<Message>,
    last_used: Instant,
}

impl ConversationManager {
    pub fn new(
        provider: Arc<dyn LmProvider>,
        system_prompt: String,
        session_timeout_secs: u64,
        max_response_tokens: u32,
    ) -> Self {
        Self {
            provider,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            session_timeout: Duration::from_secs(session_timeout_secs),
            system_prompt,
            max_response_tokens,
        }
    }

    /// Send a prompt and get the response text.
    ///
    /// Maintains per-call message history so multi-turn voice conversations
    /// carry context across utterances within a single call.
    pub async fn send(&self, call_sid: &str, prompt: &str) -> Result<String, ConversationError> {
        let mut sessions = self.sessions.lock().await;

        // Clean up expired sessions
        sessions.retain(|_, s| s.last_used.elapsed() < self.session_timeout);

        let session = sessions
            .entry(call_sid.to_string())
            .or_insert_with(|| Session {
                messages: Vec::new(),
                last_used: Instant::now(),
            });

        // Append user message
        session.messages.push(Message {
            role: Role::User,
            content: MessageContent::Text(prompt.to_string()),
        });
        session.last_used = Instant::now();

        // Clone what we need before releasing the lock
        let messages = session.messages.clone();
        drop(sessions);

        tracing::info!(call_sid, provider = self.provider.name(), "Invoking LLM");

        let response = self
            .provider
            .invoke(
                &self.system_prompt,
                &messages,
                self.max_response_tokens,
                None, // no tools for voice
            )
            .await
            .map_err(|e| ConversationError::Provider(e.to_string()))?;

        let text = response.text();

        // Append assistant response to history
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get_mut(call_sid) {
            session.messages.push(Message {
                role: Role::Assistant,
                content: MessageContent::Text(text.clone()),
            });
            session.last_used = Instant::now();
        }

        tracing::info!(call_sid, response_len = text.len(), "LLM responded");

        Ok(text)
    }

    /// Remove a session (call ended).
    pub async fn end_session(&self, call_sid: &str) {
        self.sessions.lock().await.remove(call_sid);
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConversationError {
    #[error("LLM provider error: {0}")]
    Provider(String),
}
