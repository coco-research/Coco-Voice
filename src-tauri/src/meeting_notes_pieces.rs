//! Decide one-shot vs pieces for meeting notes.
//!
//! When a meeting stops, notes are written in one model call if the whole
//! transcript fits the notes context. Otherwise the transcript is cut into
//! pieces on segment boundaries: one call per piece (map), then one merge call.
//! The context window comes from the loaded model context; no second hardcoded
//! context constant lives here.
#![cfg_attr(not(test), allow(dead_code))] // The notes runner uses this later.

use std::ops::Range;

/// Tokens reserved for the model's reply.
pub const NOTES_MAX_TOKENS: usize = 1024;
/// Overhead for ChatML turn markers, system prompt, and Qwen3 no-think tail.
pub const CHAT_TEMPLATE_OVERHEAD_TOKENS: usize = 64;
/// Per-segment overhead for the "[S<id>] <channel>: " tag and newline.
pub const SEGMENT_TAG_TOKENS: usize = 6;

/// Estimated token count of `text`: `chars() / 4`, rounded up (0 when empty).
pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

/// Room left for transcript in `n_ctx` after reserving reply tokens, chat
/// framing, a 15% tokenization safety margin on `chars / 4`, and `prompt_tokens`.
pub fn piece_budget(n_ctx: usize, prompt_tokens: usize) -> usize {
    let prompt_room = n_ctx
        .saturating_sub(NOTES_MAX_TOKENS)
        .saturating_sub(CHAT_TEMPLATE_OVERHEAD_TOKENS);
    // `chars / 4` can undercount dense punctuation/numbers by ~10-15%.
    let safe_room = prompt_room.saturating_mul(85) / 100;
    safe_room.saturating_sub(prompt_tokens)
}

/// Pack segment token counts into consecutive non-overlapping ranges covering
/// `0..segment_tokens.len()`, each with total cost `<= budget`, where each
/// segment costs `segment_tokens[i] + SEGMENT_TAG_TOKENS`.
///
/// Empty input gives an empty `Vec`. When the total cost fits `budget` the
/// result is one range `0..len` (one-shot). A single segment whose cost alone
/// exceeds `budget` gets a range of its own: segments are never split.
pub fn plan_pieces_within(segment_tokens: &[usize], budget: usize) -> Vec<Range<usize>> {
    if segment_tokens.is_empty() {
        return Vec::new();
    }
    let seg_cost = |n: usize| n.saturating_add(SEGMENT_TAG_TOKENS);
    let total = segment_tokens
        .iter()
        .fold(0usize, |acc, &n| acc.saturating_add(seg_cost(n)));
    if total <= budget {
        return vec![0..segment_tokens.len()];
    }

    let mut pieces = Vec::new();
    let mut start = 0;
    let mut used = 0usize;
    for (i, &n) in segment_tokens.iter().enumerate() {
        let cost = seg_cost(n);
        if i > start && used.saturating_add(cost) > budget {
            pieces.push(start..i);
            start = i;
            used = cost;
        } else {
            used = used.saturating_add(cost);
        }
    }
    pieces.push(start..segment_tokens.len());
    pieces
}

/// Plan the model calls for one meeting transcript.
///
/// `segment_tokens[i]` is the estimated text tokens of segment `i`; each
/// segment costs `segment_tokens[i] + SEGMENT_TAG_TOKENS`. The budget is
/// [`piece_budget`]`(n_ctx, prompt_tokens)` minus `notepad_tokens` (saturating),
/// because the notepad goes into every piece.
pub fn plan_pieces(
    segment_tokens: &[usize],
    notepad_tokens: usize,
    prompt_tokens: usize,
    n_ctx: usize,
) -> Vec<Range<usize>> {
    let budget = piece_budget(n_ctx, prompt_tokens).saturating_sub(notepad_tokens);
    plan_pieces_within(segment_tokens, budget)
}

/// True when there is exactly one piece, so notes need no merge call.
pub fn is_one_shot(pieces: &[Range<usize>]) -> bool {
    pieces.len() == 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ops::Range;

    const TEST_N_CTX: usize = 8192;

    /// What one segment adds to a piece: its text plus its tag.
    fn cost(n: usize) -> usize {
        n + SEGMENT_TAG_TOKENS
    }

    fn piece_cost(segment_tokens: &[usize], piece: &Range<usize>) -> usize {
        segment_tokens[piece.clone()].iter().map(|&n| cost(n)).sum()
    }

    #[test]
    fn estimate_rounds_up() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
    }

    #[test]
    fn budget_derives_from_passed_context_window() {
        let expected_8k =
            ((8192 - NOTES_MAX_TOKENS - CHAT_TEMPLATE_OVERHEAD_TOKENS) * 85) / 100 - 200;
        assert_eq!(piece_budget(8192, 200), expected_8k);
        let expected_4k =
            ((4096 - NOTES_MAX_TOKENS - CHAT_TEMPLATE_OVERHEAD_TOKENS) * 85) / 100 - 200;
        assert_eq!(piece_budget(4096, 200), expected_4k);
        let expected_2k =
            ((2048 - NOTES_MAX_TOKENS - CHAT_TEMPLATE_OVERHEAD_TOKENS) * 85) / 100 - 200;
        assert_eq!(piece_budget(2048, 200), expected_2k);
        assert!(piece_budget(2048, 200) < 2048 - NOTES_MAX_TOKENS - 200);
        assert_eq!(piece_budget(TEST_N_CTX, TEST_N_CTX), 0);
    }

    #[test]
    fn meeting_just_under_budget_is_one_shot() {
        let b = piece_budget(TEST_N_CTX, 200);
        let s = b / 10 - SEGMENT_TAG_TOKENS;
        assert!(10 * cost(s) <= b);
        let pieces = plan_pieces(&[s; 10], 0, 200, TEST_N_CTX);
        assert_eq!(pieces, vec![0..10]);
        assert!(is_one_shot(&pieces));
    }

    #[test]
    fn notepad_counts_against_the_budget() {
        let b = piece_budget(TEST_N_CTX, 200);
        let s = b / 10 - SEGMENT_TAG_TOKENS;
        let pieces = plan_pieces(&[s; 10], b, 200, TEST_N_CTX);
        let expected: Vec<Range<usize>> = (0..10).map(|i| i..i + 1).collect();
        assert_eq!(pieces, expected);
        assert!(!is_one_shot(&pieces));
    }

    #[test]
    fn long_meeting_never_splits_a_segment() {
        let b = piece_budget(TEST_N_CTX, 200);
        let notepad = 50;
        let s2 = b / 100;
        let segments = vec![s2; 300];
        let pieces = plan_pieces(&segments, notepad, 200, TEST_N_CTX);
        assert!(pieces.len() > 1);
        assert_eq!(pieces[0].start, 0);
        for pair in pieces.windows(2) {
            assert_eq!(pair[0].end, pair[1].start);
        }
        assert_eq!(pieces[pieces.len() - 1].end, 300);
        for piece in &pieces {
            assert!(!piece.is_empty());
            assert!(piece_cost(&segments, piece) <= b - notepad);
        }
    }

    #[test]
    fn oversized_segment_gets_its_own_piece() {
        let b = piece_budget(TEST_N_CTX, 200);
        let pieces = plan_pieces(&[10, b * 2, 10], 0, 200, TEST_N_CTX);
        assert_eq!(pieces, vec![0..1, 1..2, 2..3]);
    }

    #[test]
    fn empty_gives_no_pieces() {
        let pieces = plan_pieces(&[], 0, 200, TEST_N_CTX);
        assert!(pieces.is_empty());
        assert!(!is_one_shot(&pieces));
    }

    #[test]
    fn plan_pieces_within_covers_empty_oneshot_exact_boundary_and_oversized() {
        // Empty input -> empty pieces.
        let empty = plan_pieces_within(&[], 100);
        assert!(empty.is_empty());
        assert!(!is_one_shot(&empty));

        // Total cost (16 + 26 + 36 = 78) fits within 100 -> one-shot.
        let one_shot = plan_pieces_within(&[10, 20, 30], 100);
        assert_eq!(one_shot, vec![0..3]);
        assert!(is_one_shot(&one_shot));

        // Exact boundary: cost(44) = 50, so two fit exactly in 100 while 50 + 51 = 101 splits.
        assert_eq!(plan_pieces_within(&[44, 44, 44, 44], 100), vec![0..2, 2..4]);
        assert_eq!(
            plan_pieces_within(&[44, 45, 44], 100),
            vec![0..1, 1..2, 2..3]
        );

        // Oversized single segment gets its own piece without splitting.
        assert_eq!(
            plan_pieces_within(&[10, 200, 10], 100),
            vec![0..1, 1..2, 2..3]
        );
    }
}
