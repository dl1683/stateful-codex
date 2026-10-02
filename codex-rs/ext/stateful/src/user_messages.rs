//! The user's own messages as the host received them, kept per thread so a recorded rule
//! can be checked against what the user actually wrote.
//!
//! Only the human-origin input that started a turn enters this record (the host filters it
//! by input origin before turn start). Steering messages are left out: their items carry no
//! origin, so scheduled heartbeat input could pass as the user's. Contextual fragments such
//! as AGENTS.md text, compaction summaries and assistant answers are never user input. The
//! current turn's message is recorded at every turn start, so a cold resume still has it.

use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;

use codex_protocol::user_input::UserInput;

const MAX_MESSAGES_PER_THREAD: usize = 24;
const MAX_THREADS: usize = 64;
const MAX_MESSAGE_BYTES: usize = 16 * 1024;

/// One captured user message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UserMessage {
    pub(crate) project_id: String,
    pub(crate) turn_id: String,
    pub(crate) text: String,
}

#[derive(Default)]
struct Threads {
    messages: HashMap<String, VecDeque<UserMessage>>,
    /// Threads in first-recorded order, for bounded eviction.
    order: VecDeque<String>,
}

/// Newest-last user messages per thread, shared by the turn hooks and the record tool.
#[derive(Clone, Default)]
pub(crate) struct UserMessageRegistry {
    threads: Arc<Mutex<Threads>>,
}

impl UserMessageRegistry {
    /// Appends the text items of one turn's starting input, bounded in count and size.
    pub(crate) fn record(
        &self,
        thread_id: &str,
        project_id: &str,
        turn_id: &str,
        content: &[UserInput],
    ) {
        let text = content
            .iter()
            .filter_map(|item| match item {
                UserInput::Text { text, .. } => Some(text.as_str()),
                // `UserInput` is non-exhaustive; only text can be quoted.
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if text.trim().is_empty() {
            return;
        }
        // A long message keeps only whole lines, so no sentence can lose its qualifiers to
        // the bound; a first line longer than the bound keeps nothing.
        let end = if text.len() <= MAX_MESSAGE_BYTES {
            text.len()
        } else {
            let mut limit = MAX_MESSAGE_BYTES;
            while !text.is_char_boundary(limit) {
                limit -= 1;
            }
            match text[..limit].rfind('\n') {
                Some(newline) => newline,
                None => return,
            }
        };
        let message = UserMessage {
            project_id: project_id.to_string(),
            turn_id: turn_id.to_string(),
            text: text[..end].to_string(),
        };
        let mut threads = self
            .threads
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !threads.messages.contains_key(thread_id) {
            if threads.order.len() == MAX_THREADS
                && let Some(evicted) = threads.order.pop_front()
            {
                threads.messages.remove(&evicted);
            }
            threads.order.push_back(thread_id.to_string());
        }
        let messages = threads.messages.entry(thread_id.to_string()).or_default();
        if messages.iter().any(|existing| *existing == message) {
            return;
        }
        if messages.len() == MAX_MESSAGES_PER_THREAD {
            messages.pop_front();
        }
        messages.push_back(message);
    }

    /// The newest message of `project_id` in `thread_id` with exactly one clause containing
    /// `quote`, and that whole clause as the user wrote it.
    pub(crate) fn find(
        &self,
        thread_id: &str,
        project_id: &str,
        quote: &str,
    ) -> Option<(UserMessage, String)> {
        let threads = self
            .threads
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        threads
            .messages
            .get(thread_id)?
            .iter()
            .rev()
            .filter(|message| message.project_id == project_id)
            .find_map(|message| {
                crate::user_rules::clause_containing(&message.text, quote)
                    .map(|clause| (message.clone(), clause))
            })
    }
}

#[cfg(test)]
#[path = "user_messages_tests.rs"]
mod tests;
