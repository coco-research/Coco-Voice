//! Decide one-shot vs pieces for meeting notes.
//!
//! When a meeting stops, notes are written in one model call if the whole
//! transcript fits the notes context. Otherwise the transcript is cut into
//! pieces on segment boundaries: one call per piece (map), then one merge call.
//! The thresholds live here so the runner and its tests share one source.
#![cfg_attr(not(test), allow(dead_code))] // The notes runner uses this later.

use std::ops::Range;

/// Context the notes runner opens the local model with; dictation cleanup
/// keeps its own 2,048.
pub const NOTES_N_CTX: usize = 8192;
/// Tokens reserved for the model's reply.
pub const NOTES_MAX_TOKENS: usize = 1024;
/// Per-segment overhead for the "[S<id>] <channel>: " tag and newline.
pub const SEGMENT_TAG_TOKENS: usize = 6;

/// Estimated token count of `text`: `chars() / 4`, rounded up (0 when empty).
pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

/// Room left for transcript after the reply and the prompt, saturating at 0.
pub fn piece_budget(prompt_tokens: usize) -> usize {
    NOTES_N_CTX
        .saturating_sub(NOTES_MAX_TOKENS)
        .saturating_sub(prompt_tokens)
}

/// Plan the model calls for one meeting transcript.
///
/// `segment_tokens[i]` is the estimated text tokens of segment `i`; each
/// segment costs `segment_tokens[i] + SEGMENT_TAG_TOKENS`. The budget is
/// [`piece_budget`]`(prompt_tokens)` minus `notepad_tokens` (saturating),
/// because the notepad goes into every piece.
///
/// Empty input gives an empty `Vec`. When the total cost fits the budget the
/// result is one range `0..len` (one-shot). Otherwise segments are packed
/// greedily, in order, into consecutive non-overlapping ranges covering
/// `0..len`, each with total cost `<= budget`. A single segment whose cost
/// alone exceeds the budget gets a range of its own: segments are never split.
pub fn plan_pieces(
    segment_tokens: &[usize],
    notepad_tokens: usize,
    prompt_tokens: usize,
) -> Vec<Range<usize>> {
    if segment_tokens.is_empty() {
        return Vec::new();
    }
    let seg_cost = |n: usize| n.saturating_add(SEGMENT_TAG_TOKENS);
    let budget = piece_budget(prompt_tokens).saturating_sub(notepad_tokens);
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

/// True when there is exactly one piece, so notes need no merge call.
pub fn is_one_shot(pieces: &[Range<usize>]) -> bool {
    pieces.len() == 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ops::Range;

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
    fn budget_subtracts_reply_and_prompt() {
        assert_eq!(piece_budget(200), NOTES_N_CTX - NOTES_MAX_TOKENS - 200);
        assert_eq!(piece_budget(NOTES_N_CTX), 0);
    }

    #[test]
    fn meeting_just_under_budget_is_one_shot() {
        let b = piece_budget(200);
        let s = b / 10 - SEGMENT_TAG_TOKENS;
        assert!(10 * cost(s) <= b);
        let pieces = plan_pieces(&[s; 10], 0, 200);
        assert_eq!(pieces, vec![0..10]);
        assert!(is_one_shot(&pieces));
    }

    #[test]
    fn notepad_counts_against_the_budget() {
        let b = piece_budget(200);
        let s = b / 10 - SEGMENT_TAG_TOKENS;
        let pieces = plan_pieces(&[s; 10], b, 200);
        let expected: Vec<Range<usize>> = (0..10).map(|i| i..i + 1).collect();
        assert_eq!(pieces, expected);
        assert!(!is_one_shot(&pieces));
    }

    #[test]
    fn long_meeting_never_splits_a_segment() {
        let b = piece_budget(200);
        let notepad = 50;
        let s2 = b / 100;
        let segments = vec![s2; 300];
        let pieces = plan_pieces(&segments, notepad, 200);
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
        let b = piece_budget(200);
        let pieces = plan_pieces(&[10, b * 2, 10], 0, 200);
        assert_eq!(pieces, vec![0..1, 1..2, 2..3]);
    }

    #[test]
    fn empty_gives_no_pieces() {
        let pieces = plan_pieces(&[], 0, 200);
        assert!(pieces.is_empty());
        assert!(!is_one_shot(&pieces));
    }
}
