//! Context building under a token budget (P2-T03).
//!
//! Order of priority: the system prompt, then retrieved items (none until P7), then the most
//! recent turns that fit. The **latest user turn is always included**, truncated with a marker
//! if it alone exceeds the budget. Tokens are estimated as `ceil(chars / 4)` plus a small
//! per-message overhead until a tokenizer ADR replaces the estimate.

use loki_core::{Message, Role};
use loki_llm::ChatMessage;

/// Characters per token for the estimate. Conservative for English; most models do better.
pub const CHARS_PER_TOKEN: usize = 4;
/// Tokens charged per message for role markers and separators in chat templates.
pub const MESSAGE_OVERHEAD_TOKENS: u32 = 4;
/// Appended to a message cut to fit the budget, so the model knows text is missing.
pub const TRUNCATION_MARKER: &str = " [...truncated to fit the context window]";
/// The smallest prompt budget we will work with, so a misconfiguration can't produce an empty prompt.
pub const MIN_PROMPT_TOKENS: u32 = 64;

/// Estimated tokens for one message with `text`.
pub fn estimate_tokens(text: &str) -> u32 {
    let chars = text.chars().count();
    let tokens = chars.div_ceil(CHARS_PER_TOKEN);
    u32::try_from(tokens)
        .unwrap_or(u32::MAX)
        .saturating_add(MESSAGE_OVERHEAD_TOKENS)
}

/// How many prompt tokens a request may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextBudget {
    /// Upper bound for the estimated prompt size.
    pub max_prompt_tokens: u32,
}

impl ContextBudget {
    /// The provider's context window minus the tokens reserved for the reply, but never less
    /// than [`MIN_PROMPT_TOKENS`].
    pub fn from_limits(context_limit: u32, reserve_for_reply: u32) -> Self {
        Self {
            max_prompt_tokens: context_limit
                .saturating_sub(reserve_for_reply)
                .max(MIN_PROMPT_TOKENS),
        }
    }
}

/// The context sent to the model, plus what had to be left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltContext {
    /// Messages in chronological order, system prompt first.
    pub messages: Vec<ChatMessage>,
    /// History messages left out because they didn't fit.
    pub dropped: usize,
    /// `true` if a message had to be cut (always the system prompt or the latest user turn).
    pub truncated: bool,
    /// Estimated prompt tokens of `messages`.
    pub estimated_tokens: u32,
}

/// Cuts `text` so that a message holding it (plus the marker) costs at most `tokens`.
fn truncate_to(text: &str, tokens: u32) -> String {
    let marker_chars = TRUNCATION_MARKER.chars().count();
    let budget_chars = usize::try_from(tokens.saturating_sub(MESSAGE_OVERHEAD_TOKENS))
        .unwrap_or(usize::MAX)
        .saturating_mul(CHARS_PER_TOKEN)
        .saturating_sub(marker_chars);
    let mut cut: String = text.chars().take(budget_chars).collect();
    cut.push_str(TRUNCATION_MARKER);
    cut
}

/// Builds the bounded context for the next reply. See the module docs for the rules.
pub fn build_context(
    system: Option<&str>,
    history: &[Message],
    budget: ContextBudget,
) -> BuiltContext {
    let mut remaining = budget.max_prompt_tokens;
    let mut truncated = false;

    // 1. System prompt. It may take at most half the budget, so a long one can't crowd out the user.
    let system_message = system.filter(|s| !s.trim().is_empty()).map(|text| {
        let cap = remaining / 2;
        let content = if estimate_tokens(text) > cap {
            truncated = true;
            truncate_to(text, cap)
        } else {
            text.to_owned()
        };
        remaining = remaining.saturating_sub(estimate_tokens(&content));
        ChatMessage::new(Role::System, content)
    });

    // 2. Retrieved items: none until P7-T04.

    // 3. The latest user turn, always.
    let latest_user = history.iter().rposition(|m| m.role == Role::User);
    let mut chosen: Vec<(usize, String)> = Vec::new();
    if let Some(index) = latest_user {
        let text = &history[index].content;
        let content = if estimate_tokens(text) > remaining {
            truncated = true;
            truncate_to(text, remaining)
        } else {
            text.clone()
        };
        remaining = remaining.saturating_sub(estimate_tokens(&content));
        chosen.push((index, content));
    }

    // 4. Most recent other turns, newest first, until one doesn't fit. Stopping at the first
    //    misfit keeps a contiguous recent window rather than a patchwork of old short messages.
    let mut dropped = 0;
    let mut full = false;
    for (index, message) in history.iter().enumerate().rev() {
        if Some(index) == latest_user || message.role == Role::System {
            continue;
        }
        let cost = estimate_tokens(&message.content);
        if full || cost > remaining {
            full = true;
            dropped += 1;
            continue;
        }
        remaining -= cost;
        chosen.push((index, message.content.clone()));
    }

    chosen.sort_by_key(|(index, _)| *index);
    let mut messages: Vec<ChatMessage> = system_message.into_iter().collect();
    messages.extend(
        chosen
            .into_iter()
            .map(|(index, content)| ChatMessage::new(history[index].role, content)),
    );
    BuiltContext {
        estimated_tokens: budget.max_prompt_tokens.saturating_sub(remaining),
        messages,
        dropped,
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use loki_core::{ConversationId, MessageId, UnixMillis};

    use super::*;

    fn msg(role: Role, content: &str) -> Message {
        Message::new(
            MessageId::new(content),
            ConversationId::new("c"),
            role,
            content,
            UnixMillis(0),
        )
    }

    fn contents(built: &BuiltContext) -> Vec<&str> {
        built.messages.iter().map(|m| m.content.as_str()).collect()
    }

    #[test]
    fn estimate_is_chars_over_four_plus_overhead() {
        assert_eq!(estimate_tokens(""), MESSAGE_OVERHEAD_TOKENS);
        assert_eq!(estimate_tokens("abcd"), 1 + MESSAGE_OVERHEAD_TOKENS);
        assert_eq!(estimate_tokens("abcde"), 2 + MESSAGE_OVERHEAD_TOKENS);
    }

    #[test]
    fn budget_reserves_room_for_the_reply_with_a_floor() {
        assert_eq!(
            ContextBudget::from_limits(4096, 1024).max_prompt_tokens,
            3072
        );
        assert_eq!(
            ContextBudget::from_limits(100, 1024).max_prompt_tokens,
            MIN_PROMPT_TOKENS
        );
    }

    #[test]
    fn empty_history_gives_only_the_system_prompt() {
        let built = build_context(
            Some("be brief"),
            &[],
            ContextBudget::from_limits(4096, 1024),
        );
        assert_eq!(contents(&built), ["be brief"]);
        assert_eq!(built.messages[0].role, Role::System);
        assert_eq!(built.dropped, 0);
        assert!(!built.truncated);
    }

    #[test]
    fn everything_fits_in_order() {
        let history = [
            msg(Role::User, "a"),
            msg(Role::Assistant, "b"),
            msg(Role::User, "c"),
        ];
        let built = build_context(
            Some("sys"),
            &history,
            ContextBudget::from_limits(4096, 1024),
        );
        assert_eq!(contents(&built), ["sys", "a", "b", "c"]);
    }

    #[test]
    fn oldest_turns_are_dropped_first_and_latest_user_is_kept() {
        // Each 40-char message costs 14 tokens. Budget 64 = system "s" (5) + latest (14) + 3 more (42) = 61.
        let long = |c: char| c.to_string().repeat(40);
        let history = [
            msg(Role::User, &long('1')),
            msg(Role::Assistant, &long('2')),
            msg(Role::User, &long('3')),
            msg(Role::Assistant, &long('4')),
            msg(Role::User, &long('5')),
            msg(Role::Assistant, &long('6')),
            msg(Role::User, &long('7')),
        ];
        let built = build_context(
            Some("s"),
            &history,
            ContextBudget {
                max_prompt_tokens: 64,
            },
        );
        let kept: Vec<char> = built.messages[1..]
            .iter()
            .map(|m| m.content.chars().next().unwrap())
            .collect();
        assert_eq!(kept, ['4', '5', '6', '7']);
        assert_eq!(built.dropped, 3);
        assert!(built.estimated_tokens <= 64);
    }

    #[test]
    fn a_single_oversized_user_message_is_truncated_with_a_marker() {
        let huge = "x".repeat(10_000);
        let history = [msg(Role::User, "old"), msg(Role::User, &huge)];
        let budget = ContextBudget {
            max_prompt_tokens: 100,
        };
        let built = build_context(None, &history, budget);
        assert!(built.truncated);
        assert_eq!(built.messages.len(), 1, "no room left for older turns");
        let last = &built.messages[0].content;
        assert!(last.ends_with(TRUNCATION_MARKER));
        assert!(estimate_tokens(last) <= 100);
        assert_eq!(built.dropped, 1);
    }

    #[test]
    fn latest_user_turn_is_kept_even_when_assistant_replied_after_it() {
        let history = [msg(Role::User, "question"), msg(Role::Assistant, "partial")];
        let built = build_context(None, &history, ContextBudget::from_limits(4096, 0));
        assert_eq!(contents(&built), ["question", "partial"]);
    }

    #[test]
    fn long_system_prompt_is_capped_at_half_the_budget() {
        let system = "s".repeat(4_000);
        let built = build_context(
            Some(&system),
            &[msg(Role::User, "hi")],
            ContextBudget {
                max_prompt_tokens: 200,
            },
        );
        assert!(built.truncated);
        assert!(estimate_tokens(&built.messages[0].content) <= 100);
        assert_eq!(built.messages[1].content, "hi");
    }
}
