//! Merge the cited note lines produced for several transcript pieces.
//!
//! Contract:
//! - Two lines are duplicates when their kinds are equal and their normalised
//!   texts are equal.
//! - Normalise = lowercase, whitespace runs collapsed to one space, trimmed,
//!   trailing `.` `!` `?` `,` `;` characters removed.
//! - A duplicate group keeps the text of its first occurrence (pieces in order,
//!   lines in order) and the sorted, deduped union of all its citations.
//! - Output is ordered by each line's smallest citation ascending; ties and
//!   lines with no citation keep first-occurrence order, and lines with no
//!   citation go last.
//!
//! Each tuple is (kind, text, cited segment ids).

// The notes runner uses this later; until then only the tests read it.
#![cfg_attr(not(test), allow(dead_code))]

pub fn merge_pieces<K: Eq>(pieces: Vec<Vec<(K, String, Vec<i64>)>>) -> Vec<(K, String, Vec<i64>)> {
    // (kind, first text, normalised text, cited ids) in first-occurrence order.
    let mut groups: Vec<(K, String, String, Vec<i64>)> = Vec::new();

    for piece in pieces {
        for (kind, text, ids) in piece {
            let key = normalise(&text);
            // ponytail: linear scan, K carries Eq but no Hash bound.
            match groups.iter().position(|g| g.0 == kind && g.2 == key) {
                Some(index) => groups[index].3.extend(ids),
                None => groups.push((kind, text, key, ids)),
            }
        }
    }

    for group in &mut groups {
        group.3.sort_unstable();
        group.3.dedup();
    }

    // Stable sort by smallest citation; uncited lines (i64::MAX) go last and
    // ties keep their first-occurrence order.
    groups.sort_by_key(|g| g.3.first().copied().unwrap_or(i64::MAX));

    groups
        .into_iter()
        .map(|(kind, text, _, ids)| (kind, text, ids))
        .collect()
}

fn normalise(text: &str) -> String {
    let collapsed = text
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    collapsed
        .trim_end_matches(['.', '!', '?', ',', ';'])
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::merge_pieces;

    fn line(kind: &'static str, text: &str, ids: Vec<i64>) -> (&'static str, String, Vec<i64>) {
        (kind, text.to_string(), ids)
    }

    #[test]
    fn same_decision_in_two_pieces_merges() {
        let pieces = vec![
            vec![line("decision", "Ship the beta on Monday.", vec![4])],
            vec![line("decision", "ship the  beta on monday", vec![9, 4])],
        ];
        let expected = vec![line("decision", "Ship the beta on Monday.", vec![4, 9])];
        assert_eq!(merge_pieces(pieces), expected);
    }

    #[test]
    fn same_text_different_kind_stays_apart() {
        let pieces = vec![vec![
            line("decision", "Ship it", vec![1]),
            line("action", "Ship it", vec![2]),
        ]];
        assert_eq!(merge_pieces(pieces).len(), 2);
    }

    #[test]
    fn distinct_lines_stay() {
        let pieces = vec![vec![
            line("summary", "First line", vec![1]),
            line("summary", "Second line", vec![2]),
        ]];
        assert_eq!(merge_pieces(pieces).len(), 2);
    }

    #[test]
    fn ordered_by_first_citation() {
        let pieces = vec![
            vec![line("summary", "B", vec![7])],
            vec![line("summary", "A", vec![2]), line("summary", "C", vec![])],
        ];
        let texts: Vec<String> = merge_pieces(pieces)
            .into_iter()
            .map(|(_, t, _)| t)
            .collect();
        assert_eq!(
            texts,
            vec!["A".to_string(), "B".to_string(), "C".to_string()]
        );
    }

    #[test]
    fn empty_input_gives_empty() {
        assert!(merge_pieces::<&str>(vec![]).is_empty());
    }
}
