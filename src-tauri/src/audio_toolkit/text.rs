use natural::phonetics::soundex;
use once_cell::sync::Lazy;
use regex::Regex;
use std::sync::{Arc, Mutex, PoisonError};
use strsim::levenshtein;

/// Builds an n-gram string by cleaning and concatenating words
///
/// Strips punctuation from each word, lowercases, and joins without spaces.
/// This allows matching "Charge B" against "ChargeBee".
fn build_ngram(words: &[&str]) -> String {
    words
        .iter()
        .map(|w| build_match_key(w))
        .collect::<Vec<_>>()
        .concat()
}

fn build_match_key(word: &str) -> String {
    word.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

struct CustomWordMatchKey {
    word_index: usize,
    key: String,
}

fn build_custom_word_match_keys(word: &str, word_index: usize) -> Vec<CustomWordMatchKey> {
    let primary_key = build_match_key(word);
    let mut keys = Vec::with_capacity(2);

    if !primary_key.is_empty() {
        keys.push(CustomWordMatchKey {
            word_index,
            key: primary_key.clone(),
        });
    }

    if word.contains('&') {
        let expanded_key = build_match_key(&word.replace('&', " and "));
        if !expanded_key.is_empty() && expanded_key != primary_key {
            keys.push(CustomWordMatchKey {
                word_index,
                key: expanded_key,
            });
        }
    }

    keys
}

/// Finds the best matching custom word for a candidate string
///
/// Uses Levenshtein distance and Soundex phonetic matching to find
/// the best match above the given threshold.
///
/// # Arguments
/// * `candidate` - The cleaned/lowercased candidate string to match
/// * `custom_words` - Original custom words (for returning the replacement)
/// * `custom_word_match_keys` - Normalized custom-word keys for comparison
/// * `threshold` - Maximum similarity score to accept
///
/// # Returns
/// The best matching custom word and its score, if any match was found
fn find_best_match<'a>(
    candidate: &str,
    custom_words: &'a [String],
    custom_word_match_keys: &[CustomWordMatchKey],
    threshold: f64,
) -> Option<(&'a String, f64)> {
    if candidate.is_empty() || candidate.len() > 50 {
        return None;
    }

    let mut best_match: Option<&String> = None;
    let mut best_score = f64::MAX;

    for custom_word_key in custom_word_match_keys {
        // Skip if lengths are too different (optimization + prevents over-matching)
        // Use percentage-based check: max 25% length difference (prevents n-grams from
        // matching significantly shorter custom words, e.g., "openaigpt" vs "openai")
        let len_diff = (candidate.len() as i32 - custom_word_key.key.len() as i32).abs() as f64;
        let max_len = candidate.len().max(custom_word_key.key.len()) as f64;
        let max_allowed_diff = (max_len * 0.25).max(2.0); // At least 2 chars difference allowed
        if len_diff > max_allowed_diff {
            continue;
        }

        // Calculate Levenshtein distance (normalized by length)
        let levenshtein_dist = levenshtein(candidate, &custom_word_key.key);
        let max_len = candidate.len().max(custom_word_key.key.len()) as f64;
        let levenshtein_score = if max_len > 0.0 {
            levenshtein_dist as f64 / max_len
        } else {
            1.0
        };

        // Calculate phonetic similarity using Soundex
        let phonetic_match = soundex(candidate, &custom_word_key.key);

        // Combine scores: favor phonetic matches, but also consider string similarity
        let combined_score = if phonetic_match {
            levenshtein_score * 0.3 // Give significant boost to phonetic matches
        } else {
            levenshtein_score
        };

        // Accept if the score is good enough (configurable threshold)
        if combined_score < threshold && combined_score < best_score {
            best_match = Some(&custom_words[custom_word_key.word_index]);
            best_score = combined_score;
        }
    }

    best_match.map(|m| (m, best_score))
}

/// Applies custom word corrections to transcribed text using fuzzy matching
///
/// This function corrects words in the input text by finding the best matches
/// from a list of custom words using a combination of:
/// - Levenshtein distance for string similarity
/// - Soundex phonetic matching for pronunciation similarity
/// - N-gram matching for multi-word speech artifacts (e.g., "Charge B" -> "ChargeBee")
///
/// # Arguments
/// * `text` - The input text to correct
/// * `custom_words` - List of custom words to match against
/// * `threshold` - Maximum similarity score to accept (0.0 = exact match, 1.0 = any match)
///
/// # Returns
/// The corrected text with custom words applied
pub fn apply_custom_words(text: &str, custom_words: &[String], threshold: f64) -> String {
    if custom_words.is_empty() {
        return text.to_string();
    }

    // Pre-compute normalized comparison keys to avoid repeated allocations.
    let custom_word_match_keys: Vec<CustomWordMatchKey> = custom_words
        .iter()
        .enumerate()
        .flat_map(|(index, word)| build_custom_word_match_keys(word, index))
        .collect();

    let words: Vec<&str> = text.split_whitespace().collect();
    let mut result = Vec::new();
    let mut i = 0;

    while i < words.len() {
        let mut matched = false;

        // Try n-grams from longest (3) to shortest (1) - greedy matching
        for n in (1..=3).rev() {
            if i + n > words.len() {
                continue;
            }

            let ngram_words = &words[i..i + n];
            let ngram = build_ngram(ngram_words);

            if let Some((replacement, _score)) =
                find_best_match(&ngram, custom_words, &custom_word_match_keys, threshold)
            {
                // Extract punctuation from first and last words of the n-gram
                let (prefix, _) = extract_punctuation(ngram_words[0]);
                let (_, suffix) = extract_punctuation(ngram_words[n - 1]);

                // Preserve case from first word
                let corrected = preserve_case_pattern(ngram_words[0], replacement);

                result.push(format!("{}{}{}", prefix, corrected, suffix));
                i += n;
                matched = true;
                break;
            }
        }

        if !matched {
            result.push(words[i].to_string());
            i += 1;
        }
    }

    result.join(" ")
}

/// True when `word` carries explicit code-identifier casing (`camelCase`,
/// `PascalCase`, `snake_case`, `SCREAMING_SNAKE_CASE`, or `kebab-case`).
pub fn has_identifier_casing(word: &str) -> bool {
    let trimmed = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-');
    if trimmed.len() < 2 {
        return false;
    }
    if trimmed.contains('_') || trimmed.contains('-') {
        return true;
    }
    let mut seen_lower = false;
    let mut upper_count = 0usize;
    for ch in trimmed.chars() {
        if ch.is_lowercase() {
            seen_lower = true;
        } else if ch.is_uppercase() {
            upper_count += 1;
            if seen_lower {
                return true;
            }
        }
    }
    upper_count >= 2
}

/// Preserves the case pattern of the original word when applying a replacement
fn preserve_case_pattern(original: &str, replacement: &str) -> String {
    if original.chars().all(|c| c.is_uppercase()) {
        replacement.to_uppercase()
    } else if has_identifier_casing(replacement) {
        replacement.to_string()
    } else if original.chars().next().is_some_and(|c| c.is_uppercase()) {
        let mut chars: Vec<char> = replacement.chars().collect();
        if let Some(first_char) = chars.get_mut(0) {
            *first_char = first_char.to_uppercase().next().unwrap_or(*first_char);
        }
        chars.into_iter().collect()
    } else {
        replacement.to_string()
    }
}

/// Extracts punctuation prefix and suffix from a word
fn extract_punctuation(word: &str) -> (&str, &str) {
    let prefix_end = word
        .char_indices()
        .find(|(_, c)| c.is_alphanumeric())
        .map_or(word.len(), |(i, _)| i);
    let suffix_start = word
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_alphanumeric())
        .map_or(0, |(i, c)| i + c.len_utf8());
    (&word[..prefix_end], &word[suffix_start..])
}

/// Returns filler words appropriate for the given language code.
///
/// Some words like "um" and "ha" are real words in certain languages
/// (e.g., Portuguese "um" = "a/an", Spanish "ha" = "has"), so we only
/// include them as fillers for languages where they are truly fillers.
fn get_filler_words_for_language(lang: &str) -> &'static [&'static str] {
    let base_lang = lang.split(&['-', '_'][..]).next().unwrap_or(lang);

    match base_lang {
        "en" => &[
            "uh", "um", "uhm", "umm", "uhh", "uhhh", "ah", "hmm", "hm", "mmm", "mm", "mh", "eh",
            "ehh", "ha",
        ],
        "es" => &["ehm", "mmm", "hmm", "hm"],
        "pt" => &["ahm", "hmm", "mmm", "hm"],
        "fr" => &["euh", "hmm", "hm", "mmm"],
        "de" => &["äh", "ähm", "hmm", "hm", "mmm"],
        "it" => &["ehm", "hmm", "mmm", "hm"],
        "cs" => &["ehm", "hmm", "mmm", "hm"],
        "pl" => &["hmm", "mmm", "hm"],
        "tr" => &["hmm", "mmm", "hm"],
        "ru" => &["хм", "ммм", "hmm", "mmm"],
        "uk" => &["хм", "ммм", "hmm", "mmm"],
        "ar" => &["hmm", "mmm"],
        "ja" => &["hmm", "mmm"],
        "ko" => &["hmm", "mmm"],
        "vi" => &["hmm", "mmm", "hm"],
        "zh" => &["hmm", "mmm"],
        // Conservative universal fallback (no "um", "eh", "ha")
        _ => &[
            "uh", "uhm", "umm", "uhh", "uhhh", "ah", "hmm", "hm", "mmm", "mm", "mh", "ehh",
        ],
    }
}

/// Applies the user-defined correction map to `text`.
///
/// Each `{from, to}` pair is a deterministic, case-insensitive, whole-word
/// replacement of every occurrence of `from` with `to`. This is the final output
/// pass — it runs after LLM post-processing and after [`apply_custom_words`] — so
/// the user always gets a predictable last-word override. Pairs with an empty
/// `from` are skipped, and the `to` value is substituted literally (never treated
/// as a regex replacement template).
pub fn apply_corrections(text: &str, corrections: &[crate::settings::CorrectionPair]) -> String {
    let mut result = text.to_string();

    for pair in corrections {
        if pair.from.is_empty() {
            continue;
        }

        let pattern = format!(r"(?i)\b{}\b", regex::escape(&pair.from));
        if let Ok(re) = Regex::new(&pattern) {
            result = re
                .replace_all(&result, regex::NoExpand(pair.to.as_str()))
                .to_string();
        }
    }

    result
}

fn clean_spelling_token(tok: &str) -> &str {
    tok.trim_matches(|c: char| !c.is_alphanumeric() && !matches!(c, '\'' | '-' | '&'))
}

fn is_common_stopword(w: &str) -> bool {
    const STOPS: &[&str] = &[
        "a", "an", "the", "in", "on", "at", "to", "for", "of", "and", "or", "is", "are", "was",
        "were", "it", "this", "that",
    ];
    STOPS.contains(&w.to_ascii_lowercase().as_str())
}

/// Extracts spelling-fix offers from a post-paste / history user edit (`original` -> `edited`),
/// filtering out punctuation-only tweaks, sentence-start capitalization, stopword swaps,
/// unrelated semantic edits, words already in `existing_custom_words`, and pairs already in
/// `existing_corrections`.
pub(crate) fn suggest_spelling_corrections(
    original: &str,
    edited: &str,
    existing_custom_words: &[String],
    existing_corrections: &[crate::settings::CorrectionPair],
) -> Vec<crate::settings::CorrectionPair> {
    fn tokenize(s: &str) -> Vec<&str> {
        s.split_whitespace()
            .map(clean_spelling_token)
            .filter(|t| !t.is_empty())
            .collect()
    }
    let (orig, edit) = (tokenize(original), tokenize(edited));
    if orig.is_empty() || edit.is_empty() || orig == edit {
        return Vec::new();
    }

    let (m, n) = (orig.len(), edit.len());
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in (0..m).rev() {
        for j in (0..n).rev() {
            dp[i][j] = if orig[i].eq_ignore_ascii_case(edit[j]) {
                1 + dp[i + 1][j + 1]
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }

    let (mut anchors, mut i, mut j) = (Vec::new(), 0, 0);
    while i < m && j < n {
        if orig[i] == edit[j]
            || (orig[i].eq_ignore_ascii_case(edit[j]) && dp[i][j] == 1 + dp[i + 1][j + 1])
        {
            anchors.push((i, j));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    anchors.push((m, n));

    let mut out: Vec<crate::settings::CorrectionPair> = Vec::new();
    let mut push_new = |from: &str, to: &str| {
        let to_trimmed = to.trim();
        let in_custom_words = existing_custom_words
            .iter()
            .any(|w| w.trim().eq_ignore_ascii_case(to_trimmed));
        let in_corrections = existing_corrections
            .iter()
            .chain(out.iter())
            .any(|p| p.from.trim().eq_ignore_ascii_case(from.trim()));
        if !in_custom_words && !in_corrections {
            out.push(crate::settings::CorrectionPair {
                from: from.to_string(),
                to: to.to_string(),
            });
        }
    };

    let (mut prev_i, mut prev_j) = (0usize, 0usize);
    for (ai, aj) in anchors {
        if ai < m && aj < n && orig[ai] != edit[aj] {
            let (from, to) = (orig[ai], edit[aj]);
            let start_cap = ai == 0
                && from.eq_ignore_ascii_case(to)
                && !to.chars().skip(1).any(|c| c.is_uppercase());
            if !start_cap {
                push_new(from, to);
            }
        }
        let (o_span, e_span) = (&orig[prev_i..ai], &edit[prev_j..aj]);
        if (1..=2).contains(&o_span.len()) && (1..=2).contains(&e_span.len()) {
            let (from, to) = (o_span.join(" "), e_span.join(" "));
            let (k_from, k_to) = (build_match_key(&from), build_match_key(&to));
            let ratio =
                levenshtein(&k_from, &k_to) as f64 / k_from.len().max(k_to.len()).max(1) as f64;
            let stop_swap = o_span.len() == 1
                && e_span.len() == 1
                && is_common_stopword(&from)
                && is_common_stopword(&to);
            if !k_from.is_empty()
                && !k_to.is_empty()
                && from != to
                && !stop_swap
                && (k_from == k_to || (soundex(&k_from, &k_to) && ratio <= 0.55) || ratio <= 0.45)
            {
                push_new(&from, &to);
            }
        }
        prev_i = ai + 1;
        prev_j = aj + 1;
    }
    out
}

/// Persists an accepted spelling fix into local `settings` (`corrections` + `custom_words`)
/// so future takes apply it automatically.
pub(crate) fn remember_spelling_correction(
    settings: &mut crate::settings::AppSettings,
    from: &str,
    to: &str,
) -> bool {
    let (from, to) = (from.trim(), to.trim());
    if from.is_empty() || to.is_empty() || from == to {
        return false;
    }
    if let Some(existing) = settings
        .corrections
        .iter_mut()
        .find(|p| p.from.eq_ignore_ascii_case(from))
    {
        existing.to = to.to_string();
    } else {
        settings.corrections.push(crate::settings::CorrectionPair {
            from: from.to_string(),
            to: to.to_string(),
        });
    }
    if !from.contains(' ')
        && !to.contains(' ')
        && !settings
            .custom_words
            .iter()
            .any(|w| w.eq_ignore_ascii_case(to))
    {
        settings.custom_words.push(to.to_string());
    }
    true
}

struct CompiledSavedBlock {
    cue_key: String,
    insert_key: String,
    inline_re: Regex,
    content: String,
}

#[derive(Default)]
struct SavedBlocksCache {
    entry: Option<(
        Vec<crate::settings::SavedBlock>,
        Arc<Vec<CompiledSavedBlock>>,
    )>,
}

impl SavedBlocksCache {
    fn get_or_compile(
        &mut self,
        blocks: &[crate::settings::SavedBlock],
    ) -> Arc<Vec<CompiledSavedBlock>> {
        if let Some((cached_blocks, compiled)) = self.entry.as_ref() {
            if cached_blocks.as_slice() == blocks {
                return Arc::clone(compiled);
            }
        }

        let mut ordered: Vec<&crate::settings::SavedBlock> = blocks
            .iter()
            .filter(|b| !b.cue.trim().is_empty() && !b.content.is_empty())
            .collect();
        ordered.sort_by_key(|b| std::cmp::Reverse(b.cue.len()));

        let compiled: Vec<CompiledSavedBlock> = ordered
            .into_iter()
            .filter_map(|block| {
                let pat = format!(r"(?i)\binsert\s+{}\b[,.]?", regex::escape(block.cue.trim()));
                let inline_re = Regex::new(&pat).ok()?;
                Some(CompiledSavedBlock {
                    cue_key: build_match_key(&block.cue),
                    insert_key: build_match_key(&format!("insert {}", block.cue)),
                    inline_re,
                    content: block.content.clone(),
                })
            })
            .collect();

        let arc = Arc::new(compiled);
        self.entry = Some((blocks.to_vec(), Arc::clone(&arc)));
        arc
    }
}

static SAVED_BLOCKS_CACHE: Lazy<Mutex<SavedBlocksCache>> =
    Lazy::new(|| Mutex::new(SavedBlocksCache::default()));

fn compiled_saved_blocks(blocks: &[crate::settings::SavedBlock]) -> Arc<Vec<CompiledSavedBlock>> {
    SAVED_BLOCKS_CACHE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_compile(blocks)
}

/// Expands saved blocks stored locally on this Mac when the user speaks either
/// the standalone `<cue>` (whole utterance) or `"insert <cue>"` inline.
pub(crate) fn expand_saved_blocks(text: &str, blocks: &[crate::settings::SavedBlock]) -> String {
    if blocks.is_empty() || text.trim().is_empty() {
        return text.to_string();
    }
    let compiled = compiled_saved_blocks(blocks);
    if compiled.is_empty() {
        return text.to_string();
    }

    let norm_whole = build_match_key(text);
    for block in compiled.iter() {
        if !block.cue_key.is_empty()
            && (norm_whole == block.cue_key || norm_whole == block.insert_key)
        {
            return block.content.clone();
        }
    }

    let mut spans: Vec<(usize, usize, &str)> = Vec::new();
    for block in compiled.iter() {
        for m in block.inline_re.find_iter(text) {
            let (s, e) = (m.start(), m.end());
            if !spans.iter().any(|&(os, oe, _)| s < oe && os < e) {
                spans.push((s, e, block.content.as_str()));
            }
        }
    }
    if spans.is_empty() {
        return text.to_string();
    }
    spans.sort_by_key(|&(s, _, _)| s);
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for (s, e, replacement) in spans {
        out.push_str(&text[cursor..s]);
        out.push_str(replacement);
        cursor = e;
    }
    out.push_str(&text[cursor..]);
    out
}

static MULTI_SPACE_PATTERN: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s{2,}").unwrap());

/// Collapses repeated words (3+ repetitions) to a single instance.
/// E.g., "wh wh wh wh" -> "wh", "I I I I" -> "I"
fn collapse_stutters(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return text.to_string();
    }

    let mut result: Vec<&str> = Vec::new();
    let mut i = 0;

    while i < words.len() {
        let word = words[i];
        let word_lower = word.to_lowercase();

        if word_lower.chars().all(|c| c.is_alphabetic()) {
            // Count consecutive repetitions (case-insensitive)
            let mut count = 1;
            while i + count < words.len() && words[i + count].to_lowercase() == word_lower {
                count += 1;
            }

            // If 3+ repetitions, collapse to single instance
            if count >= 3 {
                result.push(word);
                i += count;
            } else {
                result.push(word);
                i += 1;
            }
        } else {
            result.push(word);
            i += 1;
        }
    }

    result.join(" ")
}

static LIST_ITEM_RETRACT_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i),\s*scratch\s+the\s+([^,.;!?]+?),\s*add\s+([^,.;!?]+)").unwrap());

static PUNCT_REPAIR_CUES: Lazy<Vec<Regex>> = Lazy::new(|| {
    [
        r"(?i),\s*sorry,\s*i\s+mean\s+",
        r"(?i),\s*actually\s+make\s+(?:it|that)\s+",
        r"(?i),\s*let\s+me\s+rephrase,\s*",
        r"(?i),\s*scratch\s+that,\s*",
        r"(?i),\s*never\s+mind,\s*",
        r"(?i),\s*correction,\s*",
        r"(?i),\s*no\s+wait,\s*",
        r"(?i),\s*or\s+rather,?\s+",
        r"(?i),\s*i\s+mean,?\s+",
        r"(?i),\s*actually,?\s+",
        r"(?i),\s*sorry,\s+",
        r"(?i),\s*no,\s+",
        r"(?i)\s+(?:scratch\s+that|sorry\s+i\s+mean|i\s+mean|or\s+rather|no\s+wait|let\s+me\s+rephrase)\s+",
        r"(?i)\s+actually\s+make\s+(?:it|that)\s+",
    ]
    .into_iter()
    .map(|pat| Regex::new(pat).unwrap())
    .collect()
});

static BARE_ACTUALLY_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\s+actually\s+").unwrap());

static LEADING_REPAIR_CUE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)^(?:sorry,\s*i\s+mean|actually\s+make\s+(?:it|that)|let\s+me\s+rephrase|scratch\s+that|never\s+mind|correction|no\s+wait|or\s+rather|i\s+mean)(?:[,\.\?!;:]+\s*|\s+|$)",
    )
    .unwrap()
});

static TRAILING_REPAIR_CUE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)(?:,\s*|\s+)(?:sorry,\s*i\s+mean|actually\s+make\s+(?:it|that)|let\s+me\s+rephrase|scratch\s+that|never\s+mind|correction|no\s+wait|or\s+rather|i\s+mean)[\.\?!]+\s*$",
    )
    .unwrap()
});

fn repair_cue_is_guarded(cue_lc: &str, rest: &str) -> bool {
    let rest_first = rest
        .split_whitespace()
        .next()
        .map(norm_repair_token)
        .unwrap_or_default();
    if cue_lc.contains("i mean") && matches!(rest_first.as_str(), "it" | "that" | "to") {
        return true;
    }
    if cue_lc.contains("scratch that") && matches!(rest_first.as_str(), "off" | "from") {
        return true;
    }
    if cue_lc.contains("no wait") && matches!(rest_first.as_str(), "time" | "times") {
        return true;
    }
    if cue_lc.contains("actually")
        && matches!(
            rest_first.as_str(),
            "think" | "enjoyed" | "liked" | "cheaper"
        )
    {
        return true;
    }
    false
}

fn norm_repair_token(word: &str) -> String {
    word.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn stems_match(a: &str, b: &str) -> bool {
    !a.is_empty() && (a == b || a.strip_suffix('s') == Some(b) || b.strip_suffix('s') == Some(a))
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// R1f prototype: a take-back inside one sentence needs evidence; `None` = leave the sentence as said.
// ponytail: ceiling = inside a sentence, only a word-for-word swap (same stem, same weekday/month/number kind, or a guarded one-word name swap) ending the sentence takes a sentence back; anything else is pasted as said so no word is lost; upgrade path = tagged retraction corpus or parser-based alignment.
fn merge_retracted_span(left: &str, replacement: &str) -> Option<String> {
    let left_trimmed = left.trim_end_matches(|c: char| c == ',' || c.is_whitespace());
    let left_tokens: Vec<&str> = left_trimmed.split_whitespace().collect();
    let rep_tokens: Vec<&str> = replacement.split_whitespace().collect();
    let n = rep_tokens.len();
    // No raw indexing here: dictation never panics, even if a guard below is removed.
    // `kept` is the text before the slot; the slot is the n words just before the cue.
    let (kept, slot) = left_tokens.split_at_checked(left_tokens.len().checked_sub(n)?)?;
    let first_rep = norm_repair_token(rep_tokens.first()?);
    let subject_start = first_rep.is_empty() || BRIDGE_SUBJECT_STARTS.contains(&first_rep.as_str());
    // Weekday, month or number; a name is never matched here (it is only a capitalisation guess).
    let kind = |w: &str| retraction_word_kind(w, false).filter(|k| *k != RetractionWordKind::Name);

    // The one rule: word for word. The replacement (3 words or fewer, not starting with a
    // subject word) stands for the same number of words just before the cue, and each pair lines up:
    // the same stem, or the same weekday, month or number kind (a name never lines up here).
    if n > 3 || subject_start {
        return None;
    }
    let pairs = || slot.iter().zip(rep_tokens.iter());
    let lines_up = |l: &str, r: &str| {
        stems_match(&norm_repair_token(l), &norm_repair_token(r))
            || (kind(l).is_some() && kind(l) == kind(r))
    };
    // A one-word name may stand for a name, as the bridge (R1d) allows: the slot is not the first word,
    // not the name someone is spoken to ("Thanks, Sam"), and the new name is not a question.
    let is_name = |w: &str| retraction_word_kind(w, false) == Some(RetractionWordKind::Name);
    let name_for_name = n == 1
        && !kept.is_empty()
        && pairs().all(|(l, r)| is_name(l) && is_name(r) && !r.ends_with(['?', '!']))
        && kept.last().is_none_or(|w| !w.ends_with(','));
    // At least one position is a real swap: a pure repeat ("twelve chairs, sorry, twelve chairs")
    // is not a take-back, so it is pasted as said.
    let real_swap = pairs().any(|(l, r)| norm_repair_token(l) != norm_repair_token(r));
    if real_swap && (name_for_name || pairs().all(|(l, r)| lines_up(l, r))) {
        return Some(if kept.is_empty() {
            capitalize_first(replacement)
        } else {
            format!("{} {replacement}", kept.join(" "))
        });
    }
    None
}

fn rewrite_single_sentence(sentence: &str) -> String {
    let mut cur = sentence.trim().to_string();
    if cur.is_empty() {
        return cur;
    }

    // Targeted list retraction: "Add milk, eggs and butter, scratch the butter, add cheese"
    if let Some(caps) = LIST_ITEM_RETRACT_RE.captures(&cur) {
        let full_match = caps.get(0).unwrap();
        let target = caps[1].trim();
        let added = caps[2].trim();
        if !target.is_empty() {
            let before = &cur[..full_match.start()];
            let after = &cur[full_match.end()..];
            // Closed determiner list; when `added` brings its own determiner, absorb one
            // leading determiner before `target` so the replacement does not double it.
            // ponytail: ceiling = a possessive word before the target ("her", "their") is taken along with it when the added text has its own determiner ("her whistle, scratch the whistle, add the compass" -> "the compass"); upgrade path = separate possessive vs article replacement policy.
            let added_starts_with_det = added.split_whitespace().next().is_some_and(|w| {
                matches!(
                    w.to_ascii_lowercase().as_str(),
                    "the"
                        | "a"
                        | "an"
                        | "my"
                        | "our"
                        | "your"
                        | "his"
                        | "her"
                        | "their"
                        | "this"
                        | "that"
                        | "some"
                )
            });
            let pat = if added_starts_with_det {
                format!(
                    r"(?i)\b(?:(?:the|a|an|my|our|your|his|her|their|this|that|some)\s+)?{}\b",
                    regex::escape(target)
                )
            } else {
                format!(r"(?i)\b{}\b", regex::escape(target))
            };
            if let Ok(re) = Regex::new(&pat) {
                if re.is_match(before) {
                    cur = format!("{}{}", re.replace(before, regex::NoExpand(added)), after);
                }
            }
        }
    }

    for _ in 0..4 {
        let mut earliest: Option<(usize, usize)> = None;
        for re in PUNCT_REPAIR_CUES.iter() {
            if let Some(m) = re.find(&cur) {
                if m.start() == 0 {
                    continue;
                }
                let cue_lc = m.as_str().to_ascii_lowercase();
                let rest = cur[m.end()..].trim_start();
                if repair_cue_is_guarded(&cue_lc, rest) {
                    continue;
                }
                if earliest.is_none_or(|(s, _)| m.start() < s) {
                    earliest = Some((m.start(), m.end()));
                }
            }
        }

        if earliest.is_none() {
            if let Some(m) = BARE_ACTUALLY_RE.find(&cur) {
                let prev_tok = cur[..m.start()]
                    .split_whitespace()
                    .last()
                    .unwrap_or_default();
                let next_tok = cur[m.end()..].split_whitespace().next().unwrap_or_default();
                if prev_tok.chars().any(|c| c.is_ascii_digit())
                    && next_tok.chars().any(|c| c.is_ascii_digit())
                {
                    earliest = Some((m.start(), m.end()));
                }
            }
        }

        let Some((start, end)) = earliest else {
            break;
        };
        let left = cur[..start].trim_end();
        let right_all = cur[end..].trim_start();
        if left.is_empty() || right_all.is_empty() {
            break;
        }
        let (replacement, tail) = match right_all.find(',') {
            Some(comma_idx) => (&right_all[..comma_idx], &right_all[comma_idx..]),
            None => (right_all, ""),
        };
        // The replacement must end the sentence: a word after it, comma clause or not,
        // means the sentence is pasted as said ("Call Sam, sorry, Ana, can you hear me?").
        if tail.chars().any(char::is_alphanumeric) {
            break;
        }
        let Some(merged) = merge_retracted_span(left, replacement.trim()) else {
            break;
        };
        cur = format!("{}{}", merged, tail);
    }

    cur
}

const REPAIR_CUE_HINTS: &[&str] = &[
    "scratch",
    "mean",
    "rather",
    "no wait",
    "correction",
    "rephrase",
    "never mind",
    "actually",
    "sorry,",
    ", no,",
];

pub(crate) fn has_repair_cue_hint(text: &str) -> bool {
    let lc = text.to_ascii_lowercase();
    REPAIR_CUE_HINTS.iter().any(|kw| lc.contains(kw))
}

pub(crate) fn repair_cue_count(text: &str) -> usize {
    let lc = text.to_ascii_lowercase();
    REPAIR_CUE_HINTS
        .iter()
        .map(|cue| lc.matches(cue).count())
        .sum()
}

pub(crate) fn model_kept_repair_cues(given: &str, out: &str) -> bool {
    let given_lc = given.to_ascii_lowercase();
    let out_lc = out.to_ascii_lowercase();
    REPAIR_CUE_HINTS
        .iter()
        .all(|cue| out_lc.matches(cue).count() >= given_lc.matches(cue).count())
}

fn word_tokens_for_keep_check(text: &str) -> Vec<String> {
    text.split_whitespace()
        .filter_map(|raw| {
            let normalized = raw.replace('\u{2019}', "'").to_lowercase();
            let trimmed = normalized.trim_matches(|c: char| !c.is_alphanumeric());
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .collect()
}

fn non_filler_word_counts(text: &str) -> std::collections::HashMap<String, usize> {
    let exempt = get_filler_words_for_language("en");
    let mut counts = std::collections::HashMap::new();
    for tok in word_tokens_for_keep_check(text) {
        if !exempt.contains(&tok.as_str()) {
            *counts.entry(tok).or_insert(0) += 1;
        }
    }
    counts
}

pub(crate) fn non_filler_word_count(text: &str) -> usize {
    non_filler_word_counts(text).values().sum()
}

pub(crate) fn model_kept_words(given: &str, out: &str) -> bool {
    let given_counts = non_filler_word_counts(given);
    let out_counts = non_filler_word_counts(out);
    given_counts
        .iter()
        .all(|(word, &need)| out_counts.get(word).copied().unwrap_or(0) >= need)
}

const BRIDGE_SUBJECT_STARTS: &[&str] = &[
    "i", "im", "ill", "ive", "id", "you", "he", "she", "it", "its", "we", "they", "this", "that",
    "there", "these", "those",
];

const WEEKDAY_WORDS: &[&str] = &[
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];

// "may" is intentionally excluded because it is also a common modal verb.
const MONTH_WORDS: &[&str] = &[
    "january",
    "february",
    "march",
    "april",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

const NUMBER_WORDS: &[&str] = &[
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "twenty",
    "thirty",
    "forty",
    "fifty",
    "sixty",
    "seventy",
    "eighty",
    "ninety",
    "hundred",
    "thousand",
    "noon",
    "midnight",
];

const NON_NAME_INTERJECTIONS: &[&str] = &[
    "i", "ok", "okay", "yes", "no", "yeah", "yep", "nope", "sure", "thanks", "please", "sorry",
    "hi", "hello", "hey", "wow", "oh", "right", "fine", "great", "cool",
    // R1f: capitalised discourse words after a cue are not names.
    "wait", "bye", "seriously", "really", "exactly", "anyway", "maybe", "done", "good", "true",
    "cheers", "never", "nothing", "whatever", "absolutely", "definitely", "perfect", "nice",
    "alright", "agreed", "correct", "honestly", "totally", "later", "thank",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetractionWordKind {
    Weekday,
    Month,
    Number,
    Name,
}

fn retraction_word_kind(raw: &str, sentence_first: bool) -> Option<RetractionWordKind> {
    let trimmed = raw.trim_matches(|c: char| !c.is_alphanumeric());
    if trimmed.is_empty() {
        return None;
    }
    let lc = trimmed.to_lowercase();
    if WEEKDAY_WORDS.contains(&lc.as_str()) {
        return Some(RetractionWordKind::Weekday);
    }
    if MONTH_WORDS.contains(&lc.as_str()) {
        return Some(RetractionWordKind::Month);
    }
    if lc == "may" {
        return None;
    }
    if trimmed.chars().any(|c| c.is_ascii_digit()) || NUMBER_WORDS.contains(&lc.as_str()) {
        return Some(RetractionWordKind::Number);
    }
    if !sentence_first
        && trimmed.chars().next().is_some_and(|c| c.is_uppercase())
        && !NON_NAME_INTERJECTIONS.contains(&lc.as_str())
    {
        return Some(RetractionWordKind::Name);
    }
    None
}

fn extract_bridge_replacement(source: &str) -> &str {
    let clause = source.split(',').next().unwrap_or("");
    clause
        .trim()
        .trim_end_matches(|c: char| matches!(c, '.' | '?' | '!'))
        .trim()
}

// ponytail: ceiling = across a full stop, only a same-stem last word, or a one-word weekday, month, number or name swap, joins; a name someone is spoken to is never swapped; anything else is left as said, and text is never cut; upgrade path = tagged retraction corpus or parser-based alignment if real misses show up
fn can_bridge_retraction_across_sentence_break(
    prev_sentence: &str,
    replacement: &str,
    repl_is_sentence_first: bool,
) -> bool {
    let rep_tokens: Vec<&str> = replacement.split_whitespace().collect();
    // No raw indexing: dictation never panics, even if a guard below is removed.
    let (Some(first_tok), Some(last_tok)) = (rep_tokens.first(), rep_tokens.last()) else {
        return false;
    };
    if rep_tokens.len() > 3 {
        return false;
    }

    let first_rep = norm_repair_token(first_tok);
    if first_rep.is_empty() || BRIDGE_SUBJECT_STARTS.contains(&first_rep.as_str()) {
        return false;
    }

    let prev_clean = prev_sentence
        .trim_end_matches(|c: char| matches!(c, '.' | '?' | '!' | ',') || c.is_whitespace());
    let prev_tokens: Vec<&str> = prev_clean.split_whitespace().collect();
    let Some(prev_last) = prev_tokens.last() else {
        return false;
    };

    let last_rep = norm_repair_token(last_tok);
    let last_prev = norm_repair_token(prev_last);
    if stems_match(&last_rep, &last_prev) {
        return true;
    }

    if rep_tokens.len() == 1 && prev_tokens.len() >= 2 {
        let prev_kind = retraction_word_kind(prev_last, false);
        let rep_kind = retraction_word_kind(first_tok, repl_is_sentence_first);
        // A name someone is spoken to ("Thanks, Sam.") is never swapped.
        let before_last = prev_tokens.len().checked_sub(2).and_then(|i| prev_tokens.get(i));
        let spoken_to = prev_kind == Some(RetractionWordKind::Name)
            && before_last.is_some_and(|w| w.ends_with(','));
        if prev_kind.is_some() && prev_kind == rep_kind && !spoken_to {
            return true;
        }
    }

    false
}

/// Rewrites mid-sentence spoken self-corrections (`"scratch that"`, `"I mean"`,
/// `"sorry, I mean"`, `"or rather"`, `"no wait"`, `"correction"`, `"never mind"`,
/// `"let me rephrase"`, `"actually make it"`, `"X, actually Y"`) while preserving
/// ordinary prose (`"I actually think..."`, `"I mean it"`, `"no wait times"`).
pub fn rewrite_spoken_self_corrections(text: &str) -> String {
    if text.trim().is_empty() || !has_repair_cue_hint(text) {
        return text.to_string();
    }
    let mut raw_sents: Vec<&str> = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();
    for (idx, ch) in text.char_indices() {
        if matches!(ch, '.' | '?' | '!') {
            let next_idx = idx + ch.len_utf8();
            let boundary = next_idx == text.len() || bytes[next_idx].is_ascii_whitespace();
            if boundary {
                let slice = text[start..next_idx].trim();
                if !slice.is_empty() {
                    raw_sents.push(slice);
                }
                start = next_idx;
            }
        }
    }
    if start < text.len() {
        let rem = text[start..].trim();
        if !rem.is_empty() {
            raw_sents.push(rem);
        }
    }

    // Bridge sentence breaks (`.`, `?`, `!`) placed right before or right after a repair cue
    // only when the replacement aligns with the previous sentence (`can_bridge_retraction_across_sentence_break`)
    // and the rewrite actually drops words, so ordinary multi-sentence prose stays untouched.
    let mut merged: Vec<String> = Vec::with_capacity(raw_sents.len());
    let mut original_spans: Vec<Vec<&str>> = Vec::with_capacity(raw_sents.len());
    for (i, sent) in raw_sents.iter().enumerate() {
        let has_next = i + 1 < raw_sents.len();
        let mut bridged = false;

        if let Some(prev) = merged.last_mut() {
            if let Some(m) = TRAILING_REPAIR_CUE_RE.find(prev) {
                let cue_lc = m.as_str().to_ascii_lowercase();
                let repl = extract_bridge_replacement(sent);
                if !repair_cue_is_guarded(&cue_lc, sent)
                    && can_bridge_retraction_across_sentence_break(&prev[..m.start()], repl, true)
                {
                    let prev_base = prev.trim_end_matches(|c: char| matches!(c, '.' | '?' | '!'));
                    *prev = format!("{prev_base}, {sent}");
                    bridged = true;
                }
            }
            if !bridged {
                if let Some(m) = LEADING_REPAIR_CUE_RE.find(sent) {
                    let cue_lc = m.as_str().to_ascii_lowercase();
                    let rest = sent[m.end()..].trim_start();
                    let repl_source = if rest.is_empty() {
                        raw_sents.get(i + 1).copied().unwrap_or("")
                    } else {
                        rest
                    };
                    let repl = extract_bridge_replacement(repl_source);
                    if (!rest.is_empty() || has_next)
                        && !repair_cue_is_guarded(&cue_lc, rest)
                        && can_bridge_retraction_across_sentence_break(
                            prev,
                            repl,
                            rest.is_empty(),
                        )
                    {
                        let prev_base =
                            prev.trim_end_matches(|c: char| matches!(c, '.' | '?' | '!'));
                        *prev = format!("{prev_base}, {sent}");
                        bridged = true;
                    }
                }
            }
        }

        if bridged {
            original_spans.last_mut().unwrap().push(sent);
        } else {
            merged.push((*sent).to_string());
            original_spans.push(vec![*sent]);
        }
    }

    merged
        .iter()
        .zip(original_spans.iter())
        .map(|(joined, orig)| {
            if orig.len() == 1 {
                return rewrite_single_sentence(joined);
            }
            let rewritten = rewrite_single_sentence(joined);
            if rewritten.split_whitespace().count() < joined.split_whitespace().count() {
                rewritten
            } else {
                orig.iter()
                    .map(|s| rewrite_single_sentence(s))
                    .collect::<Vec<_>>()
                    .join(" ")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

static ENGINE_LANG_TAG_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*<\|([a-zA-Z]{2,3}(?:-[a-zA-Z]+)?)\|>\s*").unwrap());

fn detect_script_or_lexicon_language(text: &str) -> &'static str {
    let (mut kana, mut hangul, mut han, mut cyr, mut uk_cyr, mut arab, mut hebr, mut deva) =
        (0u32, 0u32, 0u32, 0u32, 0u32, 0u32, 0u32, 0u32);
    for c in text.chars() {
        match c {
            '\u{3040}'..='\u{30FF}' => kana += 1,
            '\u{AC00}'..='\u{D7AF}' => hangul += 1,
            '\u{4E00}'..='\u{9FFF}' => han += 1,
            'і' | 'ї' | 'є' | 'ґ' | 'І' | 'Ї' | 'Є' | 'Ґ' => {
                cyr += 1;
                uk_cyr += 1;
            }
            '\u{0400}'..='\u{04FF}' => cyr += 1,
            '\u{0600}'..='\u{06FF}' => arab += 1,
            '\u{0590}'..='\u{05FF}' => hebr += 1,
            '\u{0900}'..='\u{097F}' => deva += 1,
            _ => {}
        }
    }
    if kana > 0 {
        return "ja";
    }
    if hangul > 0 {
        return "ko";
    }
    if han > 0 {
        return "zh";
    }
    if cyr > 0 {
        return if uk_cyr > 0 { "uk" } else { "ru" };
    }
    if arab > 0 {
        return "ar";
    }
    if hebr > 0 {
        return "he";
    }
    if deva > 0 {
        return "hi";
    }

    const LEX: &[(&str, &str)] = &[
        (
            "pt",
            " um uma não para com gato bonito você está muito isso obrigado ",
        ),
        (
            "es",
            " el la los las una por para sido buen día hola qué pero este esta ",
        ),
        (
            "fr",
            " le les des une dans pour avec est pas bonjour tout monde très nous ",
        ),
        (
            "de",
            " der die das und ist nicht mit ein eine sehr gut wir auf äh ähm ",
        ),
        (
            "it",
            " il gli una che non per sono questo molto ciao tutti ",
        ),
        (
            "en",
            " the and is are was this that with for you have from they would think ",
        ),
    ];

    let words: Vec<String> = text
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphabetic()).to_lowercase())
        .filter(|w| !w.is_empty())
        .collect();
    let lc = text.to_lowercase();
    let mut best = ("auto", 0i32);
    for (lang, vocab) in LEX {
        let mut score = words
            .iter()
            .filter(|w| vocab.contains(&format!(" {w} ")))
            .count() as i32
            * 2;
        score += match *lang {
            "es" if "ñ¿¡".chars().any(|c| lc.contains(c)) => 3,
            "pt" if "ãõ".chars().any(|c| lc.contains(c)) => 3,
            "de" if "ßäöü".chars().any(|c| lc.contains(c)) => 2,
            "fr" if "çœ".chars().any(|c| lc.contains(c)) || lc.contains("euh") => 2,
            _ => 0,
        };
        if score > best.1 {
            best = (lang, score);
        }
    }
    best.0
}

/// Strips optional leading engine language tags (`<|es|>`) and resolves the
/// effective language for a single take when `requested_language` is `"auto"`.
pub fn resolve_take_language(raw: &str, requested_language: &str) -> (String, String) {
    let (cleaned, tag_lang) = match ENGINE_LANG_TAG_RE.captures(raw) {
        Some(caps) => {
            let m = caps.get(0).unwrap();
            (raw[m.end()..].to_string(), Some(caps[1].to_lowercase()))
        }
        None => (raw.to_string(), None),
    };
    let req = requested_language.trim();
    if !req.is_empty() && !req.eq_ignore_ascii_case("auto") {
        return (cleaned, req.to_string());
    }
    let detected = tag_lang.unwrap_or_else(|| detect_script_or_lexicon_language(&cleaned).into());
    (cleaned, detected)
}

/// Filters transcription output by removing filler words and stutter artifacts.
///
/// This function cleans up raw transcription text by:
/// 1. Removing filler words based on the app language (or custom list)
/// 2. Collapsing repeated word stutters (e.g., "wh wh wh" -> "wh")
/// 3. Cleaning up excess whitespace
///
/// # Arguments
/// * `text` - The raw transcription text to filter
/// * `lang` - The app language code (e.g., "en", "pt-BR") used to select filler words
/// * `custom_filler_words` - Optional user-provided filler word list. `Some(vec)` overrides
///   language defaults; `Some(empty vec)` disables filtering; `None` uses language defaults.
///
/// # Returns
/// The filtered text with filler words and stutters removed
/// Resolves which language's filler list to apply (`AC-P20`):
/// 1. Spoken `selected_language` when explicit (non-empty and not `"auto"`).
/// 2. Detected language from the ASR engine when available.
/// 3. Fallback to UI `app_language`.
pub fn resolve_filler_language<'a>(
    selected_language: &'a str,
    detected_language: Option<&'a str>,
    app_language: &'a str,
) -> &'a str {
    let sel = selected_language.trim();
    if !sel.is_empty() && !sel.eq_ignore_ascii_case("auto") {
        return sel;
    }
    if let Some(det) = detected_language.map(str::trim).filter(|s| !s.is_empty()) {
        return det;
    }
    app_language
}

/// Filters transcription output with independent toggles for filler/stutter
/// removal (`remove_fillers`) and spoken self-correction rewrites
/// (`rewrite_self_corrections`), matching Plan C `I3` and `AC-P20`.
pub fn filter_transcription_output_with_policy(
    text: &str,
    lang: &str,
    custom_filler_words: &Option<Vec<String>>,
    remove_fillers: bool,
    rewrite_self_corrections: bool,
) -> String {
    let mut filtered = text.to_string();

    if remove_fillers {
        // Build filler patterns from custom list or language defaults
        let patterns: Vec<Regex> = match custom_filler_words {
            Some(words) => words
                .iter()
                .filter_map(|word| {
                    Regex::new(&format!(r"(?i)\b{}\b[,.]?", regex::escape(word))).ok()
                })
                .collect(),
            None => get_filler_words_for_language(lang)
                .iter()
                .map(|word| Regex::new(&format!(r"(?i)\b{}\b[,.]?", regex::escape(word))).unwrap())
                .collect(),
        };

        for pattern in &patterns {
            filtered = pattern.replace_all(&filtered, "").to_string();
        }

        // Collapse repeated 1-2 letter words (stutter artifacts like "wh wh wh wh")
        filtered = collapse_stutters(&filtered);
    }

    if rewrite_self_corrections {
        // Rewrite mid-sentence spoken self-corrections on the main dictation path
        filtered = rewrite_spoken_self_corrections(&filtered);
    }

    // Clean up multiple spaces to single space
    filtered = MULTI_SPACE_PATTERN.replace_all(&filtered, " ").to_string();

    // Trim leading/trailing whitespace
    filtered.trim().to_string()
}

pub fn filter_transcription_output(
    text: &str,
    lang: &str,
    custom_filler_words: &Option<Vec<String>>,
) -> String {
    filter_transcription_output_with_policy(text, lang, custom_filler_words, true, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_custom_words_exact_match() {
        let text = "hello world";
        let custom_words = vec!["Hello".to_string(), "World".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert_eq!(result, "Hello World");
    }

    #[test]
    fn test_apply_custom_words_fuzzy_match() {
        let text = "helo wrold";
        let custom_words = vec!["hello".to_string(), "world".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_preserve_case_pattern() {
        assert_eq!(preserve_case_pattern("HELLO", "world"), "WORLD");
        assert_eq!(preserve_case_pattern("Hello", "world"), "World");
        assert_eq!(preserve_case_pattern("hello", "WORLD"), "WORLD");
    }

    #[test]
    fn test_extract_punctuation() {
        assert_eq!(extract_punctuation("hello"), ("", ""));
        assert_eq!(extract_punctuation("!hello?"), ("!", "?"));
        assert_eq!(extract_punctuation("...hello..."), ("...", "..."));
    }

    #[test]
    fn test_empty_custom_words() {
        let text = "hello world";
        let custom_words = vec![];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_filter_filler_words() {
        let text = "So uhm I was thinking uh about this";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "So I was thinking about this");
    }

    #[test]
    fn test_filter_filler_words_case_insensitive() {
        let text = "UHM this is UH a test";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "this is a test");
    }

    #[test]
    fn test_filter_filler_words_with_punctuation() {
        let text = "Well, uhm, I think, uh. that's right";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "Well, I think, that's right");
    }

    #[test]
    fn test_filter_cleans_whitespace() {
        let text = "Hello    world   test";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "Hello world test");
    }

    #[test]
    fn test_filter_trims() {
        let text = "  Hello world  ";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "Hello world");
    }

    #[test]
    fn test_filter_combined() {
        let text = "  Uhm, so I was, uh, thinking about this  ";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "so I was, thinking about this");
    }

    #[test]
    fn test_filter_preserves_valid_text() {
        let text = "This is a completely normal sentence.";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "This is a completely normal sentence.");
    }

    #[test]
    fn test_filter_stutter_collapse() {
        let text = "w wh wh wh wh wh wh wh wh wh why";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "w wh why");
    }

    #[test]
    fn test_filter_stutter_short_words() {
        let text = "I I I I think so so so so";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "I think so");
    }

    #[test]
    fn test_filter_stutter_longer_words() {
        let text = "Check data doc doc doc doc documentation.";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "Check data doc documentation.");
    }

    #[test]
    fn test_filter_stutter_mixed_case() {
        let text = "No NO no NO no";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "No");
    }

    #[test]
    fn test_filter_stutter_preserves_two_repetitions() {
        let text = "no no is fine";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "no no is fine");
    }

    #[test]
    fn test_filter_english_removes_um() {
        let text = "um I think um this is good";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "I think this is good");
    }

    #[test]
    fn test_filter_portuguese_preserves_um() {
        // "um" means "a/an" in Portuguese
        let text = "um gato bonito";
        let result = filter_transcription_output(text, "pt", &None);
        assert_eq!(result, "um gato bonito");
    }

    #[test]
    fn test_filter_spanish_preserves_ha() {
        // "ha" means "has" in Spanish
        let text = "ha sido un buen día";
        let result = filter_transcription_output(text, "es", &None);
        assert_eq!(result, "ha sido un buen día");
    }

    #[test]
    fn test_filter_language_code_with_region() {
        // "pt-BR" should normalize to "pt"
        let text = "um gato bonito";
        let result = filter_transcription_output(text, "pt-BR", &None);
        assert_eq!(result, "um gato bonito");
    }

    #[test]
    fn test_filter_custom_filler_words_override() {
        let custom = Some(vec!["okay".to_string(), "right".to_string()]);
        let text = "okay so I think right this works";
        let result = filter_transcription_output(text, "en", &custom);
        assert_eq!(result, "so I think this works");
    }

    #[test]
    fn test_filter_custom_filler_words_empty_disables() {
        let custom = Some(vec![]);
        let text = "So uhm I was thinking uh about this";
        let result = filter_transcription_output(text, "en", &custom);
        // No filler words removed since custom list is empty
        assert_eq!(result, "So uhm I was thinking uh about this");
    }

    #[test]
    fn test_filter_unknown_language_uses_fallback() {
        let text = "uh I think uhm this works";
        let result = filter_transcription_output(text, "xx", &None);
        assert_eq!(result, "I think this works");
    }

    #[test]
    fn test_filter_fallback_does_not_remove_um() {
        // Fallback (unknown language) should not remove "um" since it's a real word in some languages
        let text = "um I think this works";
        let result = filter_transcription_output(text, "xx", &None);
        assert_eq!(result, "um I think this works");
    }

    #[test]
    fn test_apply_custom_words_ngram_two_words() {
        let text = "il cui nome è Charge B, che permette";
        let custom_words = vec!["ChargeBee".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert!(result.contains("ChargeBee,"));
        assert!(!result.contains("Charge B"));
    }

    #[test]
    fn test_apply_custom_words_ngram_three_words() {
        let text = "use Chat G P T for this";
        let custom_words = vec!["ChatGPT".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert!(result.contains("ChatGPT"));
    }

    #[test]
    fn test_apply_custom_words_prefers_longer_ngram() {
        let text = "Open AI GPT model";
        let custom_words = vec!["OpenAI".to_string(), "GPT".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert_eq!(result, "OpenAI GPT model");
    }

    #[test]
    fn test_apply_custom_words_ngram_preserves_case() {
        let text = "CHARGE B is great";
        let custom_words = vec!["ChargeBee".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert!(result.contains("CHARGEBEE"));
    }

    #[test]
    fn test_apply_custom_words_ngram_with_spaces_in_custom() {
        // Custom word with space should also match against split words
        let text = "using Mac Book Pro";
        let custom_words = vec!["MacBook Pro".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert!(result.contains("MacBook"));
    }

    #[test]
    fn test_apply_custom_words_trailing_number_not_doubled() {
        // Verify that trailing non-alpha chars (like numbers) aren't double-counted
        // between build_ngram stripping them and extract_punctuation capturing them
        let text = "use GPT4 for this";
        let custom_words = vec!["GPT-4".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5);
        // Should NOT produce "GPT-44" (double-counting the trailing 4)
        assert!(
            !result.contains("GPT-44"),
            "got double-counted result: {}",
            result
        );
    }

    #[test]
    fn test_apply_custom_words_matches_ampersand_word() {
        let text = "send it to RD for review";
        let custom_words = vec!["R&D".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.18);
        assert_eq!(result, "send it to R&D for review");
    }

    #[test]
    fn test_apply_custom_words_matches_spoken_ampersand_word() {
        let text = "send it to R and D for review";
        let custom_words = vec!["R&D".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.18);
        assert_eq!(result, "send it to R&D for review");
    }

    #[test]
    fn test_apply_custom_words_preserves_ampersand_word() {
        let text = "send it to R&D for review";
        let custom_words = vec!["R&D".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.18);
        assert_eq!(result, "send it to R&D for review");
    }

    fn pair(from: &str, to: &str) -> crate::settings::CorrectionPair {
        crate::settings::CorrectionPair {
            from: from.to_string(),
            to: to.to_string(),
        }
    }

    #[test]
    fn test_apply_corrections_case_insensitive_whole_word() {
        let text = "I use github and Github and GITHUB daily";
        let corrections = vec![pair("github", "GitHub")];
        let result = apply_corrections(text, &corrections);
        assert_eq!(result, "I use GitHub and GitHub and GitHub daily");
    }

    #[test]
    fn test_apply_corrections_respects_word_boundaries() {
        // Should not touch "cat" inside "category".
        let text = "the cat sat in the category";
        let corrections = vec![pair("cat", "dog")];
        let result = apply_corrections(text, &corrections);
        assert_eq!(result, "the dog sat in the category");
    }

    #[test]
    fn test_apply_corrections_multi_word_from() {
        let text = "ship it to new york tomorrow";
        let corrections = vec![pair("new york", "NYC")];
        let result = apply_corrections(text, &corrections);
        assert_eq!(result, "ship it to NYC tomorrow");
    }

    #[test]
    fn test_apply_corrections_literal_replacement() {
        // A "$1" in the replacement must not be treated as a regex capture group.
        let text = "the price is X";
        let corrections = vec![pair("X", "$1")];
        let result = apply_corrections(text, &corrections);
        assert_eq!(result, "the price is $1");
    }

    #[test]
    fn test_apply_corrections_empty_and_noop() {
        assert_eq!(apply_corrections("hello world", &[]), "hello world");
        // An empty `from` is skipped rather than matching everything.
        let corrections = vec![pair("", "x")];
        assert_eq!(
            apply_corrections("hello world", &corrections),
            "hello world"
        );
    }

    #[test]
    fn post_paste_correction_offers_spelling_fix_and_applies_on_next_take() {
        let mut settings = crate::settings::get_default_settings();
        let offers = suggest_spelling_corrections(
            "Deploy it to kubernettis with Charge B.",
            "Deploy it to Kubernetes with ChargeBee!",
            &settings.custom_words,
            &settings.corrections,
        );
        assert_eq!(offers.len(), 2);
        assert_eq!(
            (offers[0].from.as_str(), offers[0].to.as_str()),
            ("kubernettis", "Kubernetes")
        );
        assert_eq!(
            (offers[1].from.as_str(), offers[1].to.as_str()),
            ("Charge B", "ChargeBee")
        );

        // Punctuation-only edits, sentence-start capitals, and unrelated word swaps make no offer.
        assert!(suggest_spelling_corrections("hello world", "Hello world!", &[], &[]).is_empty());
        assert!(
            suggest_spelling_corrections("ship on Monday", "ship on Friday", &[], &[]).is_empty()
        );

        for o in &offers {
            assert!(remember_spelling_correction(&mut settings, &o.from, &o.to));
        }
        // Already-remembered pair is not re-offered.
        assert!(suggest_spelling_corrections(
            "Deploy it to kubernettis",
            "Deploy it to Kubernetes",
            &settings.custom_words,
            &settings.corrections,
        )
        .is_empty());

        // Subsequent take automatically applies both custom_words and corrections.
        let next_raw = "kubernettis and charge b are live";
        let after_words = apply_custom_words(next_raw, &settings.custom_words, 0.18);
        let next_final = apply_corrections(&after_words, &settings.corrections);
        assert_eq!(next_final, "Kubernetes and ChargeBee are live");
    }

    #[test]
    fn spoken_cue_inserts_saved_block_and_leaves_ordinary_mentions_intact() {
        let blocks = vec![
            crate::settings::SavedBlock {
                id: "b1".into(),
                cue: "calendar link".into(),
                content: "https://example.com/team".into(),
            },
            crate::settings::SavedBlock {
                id: "b2".into(),
                cue: "sign off".into(),
                content: "Best,\nTeam".into(),
            },
        ];
        assert_eq!(
            expand_saved_blocks("Calendar link.", &blocks),
            "https://example.com/team"
        );
        assert_eq!(
            expand_saved_blocks("Pick a slot here: insert calendar link.", &blocks),
            "Pick a slot here: https://example.com/team"
        );
        // Ordinary prose mentioning the words without "insert" is untouched.
        assert_eq!(
            expand_saved_blocks("Check the calendar link on page two.", &blocks),
            "Check the calendar link on page two."
        );
        assert!(crate::settings::get_default_settings()
            .saved_blocks
            .is_empty());
    }

    #[test]
    fn existing_custom_words_suppresses_duplicate_spelling_offer() {
        let custom_words = vec!["  kubernetes  ".to_string()];
        let offers = suggest_spelling_corrections(
            "Deploy it to kubernettis with Charge B.",
            "Deploy it to Kubernetes with ChargeBee!",
            &custom_words,
            &[],
        );
        assert_eq!(offers.len(), 1);
        assert_eq!(
            (offers[0].from.as_str(), offers[0].to.as_str()),
            ("Charge B", "ChargeBee")
        );
    }

    #[test]
    fn saved_block_content_is_preserved_verbatim_after_word_and_correction_rewrites() {
        let custom_words = vec!["Calendar".to_string()];
        let corrections = vec![pair("team", "squad"), pair("kalendar", "calendar")];
        let blocks = vec![crate::settings::SavedBlock {
            id: "b1".into(),
            cue: "calendar link".into(),
            content: "Meet the team at https://example.com/team".into(),
        }];

        // Mirrors the dictation pipeline order: apply_custom_words -> apply_corrections -> expand_saved_blocks.
        let raw = "Talk to the team and insert kalendar link.";
        let after_words = apply_custom_words(raw, &custom_words, 0.18);
        let after_corrections = apply_corrections(&after_words, &corrections);
        let final_text = expand_saved_blocks(&after_corrections, &blocks);
        assert_eq!(
            final_text,
            "Talk to the squad and Meet the team at https://example.com/team"
        );
    }

    #[test]
    fn spoken_self_corrections_pass_backtrack_30_fixture_and_unpunctuated_cues() {
        // Under R1f v11, only the 7 proven word-for-word / list-item take-backs retract;
        // the remaining 15 take-backs and all 8 keeps are pasted byte-identical as said (0 words lost).
        const PROVEN_RETRACT_IDS: &[&str] = &["r02", "r06", "r08", "r13", "r14", "r15", "r21"];
        let raw_jsonl = include_str!("../../../docs/eval/backtrack-30.jsonl");
        let mut count = 0usize;
        let mut right_count = 0usize;
        let mut as_said_count = 0usize;
        for line in raw_jsonl.lines().filter(|l| !l.trim().is_empty()) {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            let id = row["id"].as_str().unwrap();
            let input = row["input"].as_str().unwrap();
            let out = filter_transcription_output(input, "en", &None);
            let out_lc = out.to_lowercase();
            if PROVEN_RETRACT_IDS.contains(&id) {
                for group in row["keep"].as_array().unwrap() {
                    let alts: Vec<&str> = group.as_str().unwrap().split('|').collect();
                    assert!(
                        alts.iter().any(|alt| out_lc.contains(&alt.to_lowercase())),
                        "row {id} lost required token {alts:?} in '{out_lc}'"
                    );
                }
                for dropped in row["drop"].as_array().unwrap() {
                    let d = dropped.as_str().unwrap().to_lowercase();
                    let re = Regex::new(&format!(r"\b{}\b", regex::escape(&d))).unwrap();
                    assert!(
                        !re.is_match(&out_lc),
                        "row {id} kept retracted token '{d}' in '{out_lc}'"
                    );
                }
                right_count += 1;
            } else {
                assert_eq!(out, input, "row {id} should be pasted as said under R1f v11");
                as_said_count += 1;
            }
            count += 1;
        }
        assert_eq!(count, 30);
        assert_eq!(right_count, 7);
        assert_eq!(as_said_count, 23);
        // Unpunctuated raw STT cues on the main key:
        assert_eq!(
            filter_transcription_output("Let's do coffee at 2 actually 3", "en", &None),
            "Let's do coffee at 3"
        );
        assert_eq!(
            filter_transcription_output("The gate is B12 actually C7", "en", &None),
            "The gate is C7"
        );
    }

    #[test]
    fn saved_blocks_regex_cache_reuses_compiled_rules_until_blocks_change() {
        let mut cache = SavedBlocksCache::default();
        let blocks_a = vec![crate::settings::SavedBlock {
            id: "cache-1".into(),
            cue: "status link".into(),
            content: "https://example.com/status".into(),
        }];
        let first = cache.get_or_compile(&blocks_a);
        let second = cache.get_or_compile(&blocks_a);
        assert!(Arc::ptr_eq(&first, &second));

        let blocks_b = vec![crate::settings::SavedBlock {
            id: "cache-1".into(),
            cue: "status link".into(),
            content: "https://example.com/status-v2".into(),
        }];
        let rebuilt = cache.get_or_compile(&blocks_b);
        assert!(!Arc::ptr_eq(&first, &rebuilt));
        assert_eq!(
            expand_saved_blocks("Check insert status link.", &blocks_b),
            "Check https://example.com/status-v2"
        );
    }

    #[test]
    fn auto_detect_resolves_language_per_take_and_preserves_language_safe_words() {
        let cases = [
            ("um gato bonito para você", "pt", "um gato bonito para você"),
            (
                "ha sido un buen día para todos",
                "es",
                "ha sido un buen día para todos",
            ),
            ("euh bonjour tout le monde", "fr", "bonjour tout le monde"),
            ("ähm das ist sehr gut", "de", "das ist sehr gut"),
            ("<|ja|> こんにちは世界", "ja", "こんにちは世界"),
            (
                "um the release is ready for you",
                "en",
                "the release is ready for you",
            ),
        ];
        for (raw, expected_lang, expected_text) in cases {
            let (clean, detected) = resolve_take_language(raw, "auto");
            assert_eq!(detected, expected_lang, "input: {raw}");
            assert_eq!(
                filter_transcription_output(&clean, &detected, &None),
                expected_text
            );
        }
        // Explicit language choice overrides auto-detect.
        assert_eq!(resolve_take_language("hello world", "fr").1, "fr");
    }

    #[test]
    fn filler_policy_respects_spoken_language_and_flag_without_disabling_self_corrections() {
        // AC-P20: Spanish spoken take with English UI strips Spanish filler ("ehm").
        let lang_es = resolve_filler_language("es", None, "en");
        assert_eq!(lang_es, "es");
        let es_out =
            filter_transcription_output_with_policy("ehm hola mundo", lang_es, &None, true, true);
        assert_eq!(es_out, "hola mundo");

        // Reverse: English spoken take with Spanish UI strips English filler ("um").
        let lang_en = resolve_filler_language("en", None, "es");
        assert_eq!(lang_en, "en");
        let en_out =
            filter_transcription_output_with_policy("um hello world", lang_en, &None, true, true);
        assert_eq!(en_out, "hello world");

        // Auto falls back to detected language, then app_language.
        assert_eq!(resolve_filler_language("auto", Some("fr"), "en"), "fr");
        assert_eq!(resolve_filler_language("auto", None, "de"), "de");

        // remove_fillers=false keeps fillers & stutters, but still rewrites self-corrections.
        let kept_fillers = filter_transcription_output_with_policy(
            "um let us meet Monday scratch that Tuesday",
            "en",
            &None,
            false,
            true,
        );
        assert!(kept_fillers.contains("um"));
        assert!(kept_fillers.contains("Tuesday"));
        assert!(!kept_fillers.contains("scratch that"));

        // Cleanup Off: both false preserves fillers and self-correction phrases verbatim.
        let off_out = filter_transcription_output_with_policy(
            "um Monday scratch that Tuesday",
            "en",
            &None,
            false,
            false,
        );
        assert_eq!(off_out, "um Monday scratch that Tuesday");
    }

    #[test]
    fn r1b_cross_sentence_bridge_rewrites_aligned_retractions_and_preserves_ordinary_sentences() {
        let must_take_back = [
            (
                "meeting on tuesday, scratch that, wednesday",
                "wednesday",
                "tuesday",
                "scratch that",
            ),
            (
                "Meeting on Tuesday. Scratch that, Wednesday.",
                "wednesday",
                "tuesday",
                "scratch that",
            ),
            (
                "Meeting on Tuesday. Scratch that. Wednesday.",
                "wednesday",
                "tuesday",
                "scratch that",
            ),
            (
                "Meeting on Tuesday, scratch that. Wednesday.",
                "wednesday",
                "tuesday",
                "scratch that",
            ),
            (
                "Meeting on Tuesday. I mean Wednesday.",
                "wednesday",
                "tuesday",
                "i mean",
            ),
            (
                "Send it to Priya. Sorry, I mean Jordan.",
                "jordan",
                "priya",
                "i mean",
            ),
        ];
        for (input, keep, drop_old, drop_cue) in must_take_back {
            let out = rewrite_spoken_self_corrections(input);
            let lc = out.to_ascii_lowercase();
            assert!(
                lc.contains(keep) && !lc.contains(drop_old) && !lc.contains(drop_cue),
                "expected retraction for {input:?}, got {out:?}"
            );
        }

        let must_not_change = [
            "I'll be late. Sorry, traffic is bad.",
            "We shipped it. Actually, the tests were green.",
            "The demo went well. Actually, it went great.",
            "Thanks for the notes. I mean, they really helped.",
            "Send it to Priya. Correction: the deck is final.",
            "Is it ready? Actually, never mind.",
            "Book the room. No wait, I'll do it myself.",
            "Call me later. Sorry, I'm in a meeting.",
            "We shipped it. No wait, not yet.",
            "I mean it.",
            "We should go. I mean it.",
            "Scratch that off the list.",
            "Don't forget. Scratch that off the list.",
            "Good news. No wait times today.",
            "That was fun. Never mind the rain.",
            "Got it. Or rather, we got most of it.",
        ];
        for input in must_not_change {
            assert_eq!(
                rewrite_spoken_self_corrections(input),
                input,
                "ordinary sentence altered: {input:?}"
            );
        }

        // Single-sentence comma behavior stays unchanged from X2.
        assert_eq!(
            rewrite_spoken_self_corrections("Meet at 3, sorry, 4."),
            "Meet at 4."
        );
        assert_eq!(
            rewrite_spoken_self_corrections("Send it to Priya, actually, Jordan."),
            "Send it to Jordan."
        );
    }

    #[test]
    fn r1c_same_kind_bridge_matches_typed_swaps_and_blocks_non_names() {
        // Same-kind single-word swaps join across a sentence break:
        assert!(can_bridge_retraction_across_sentence_break(
            "Meeting on Tuesday.",
            "Wednesday",
            false
        ));
        assert!(can_bridge_retraction_across_sentence_break(
            "Meeting on Tuesday.",
            "Wednesday",
            true
        ));
        assert!(can_bridge_retraction_across_sentence_break(
            "The deadline is 3rd.",
            "5th",
            false
        ));
        assert!(can_bridge_retraction_across_sentence_break(
            "The report is for Q3.",
            "Q4",
            false
        ));
        assert!(can_bridge_retraction_across_sentence_break(
            "Send it to Priya.",
            "Jordan",
            false
        ));

        // Non-names, "May", and sentence-first capitalized words do NOT join:
        assert_eq!(retraction_word_kind("OK", false), None);
        assert_eq!(retraction_word_kind("Yes", false), None);
        assert_eq!(retraction_word_kind("May", false), None);
        assert_eq!(retraction_word_kind("Okay", true), None);
        assert_eq!(retraction_word_kind("Jordan", true), None);

        assert!(!can_bridge_retraction_across_sentence_break(
            "It's on GitHub.",
            "OK",
            false
        ));
        assert!(!can_bridge_retraction_across_sentence_break(
            "The code is in Rust.",
            "Yes",
            false
        ));
        assert!(!can_bridge_retraction_across_sentence_break(
            "It's in May.",
            "April",
            false
        ));
        assert!(!can_bridge_retraction_across_sentence_break(
            "Send it to Priya.",
            "Okay",
            true
        ));
        assert!(!can_bridge_retraction_across_sentence_break(
            "I'll send it to you.",
            "tomorrow",
            false
        ));

        // R1d: a name spoken to (preceded by a comma) never joins; direct name swap still joins.
        assert!(!can_bridge_retraction_across_sentence_break(
            "Thanks, Sam.",
            "Thanks",
            false
        ));
        assert_eq!(
            rewrite_spoken_self_corrections("Thanks, Sam. I mean, Thanks."),
            "Thanks, Sam. I mean, Thanks."
        );
        assert!(!can_bridge_retraction_across_sentence_break(
            "Nice work, Ana.",
            "Bravo",
            false
        ));
        assert_eq!(
            rewrite_spoken_self_corrections("Nice work, Ana. I mean, Bravo."),
            "Nice work, Ana. I mean, Bravo."
        );
        assert!(can_bridge_retraction_across_sentence_break(
            "Call Mom.",
            "Dad",
            false
        ));
        assert_eq!(
            rewrite_spoken_self_corrections("Call Mom. No wait, Dad."),
            "Call Dad."
        );
    }

    #[test]
    fn model_kept_repair_cues_checks_each_cue_count_case_insensitively() {
        // Fewer occurrences of any cue -> false.
        assert!(!model_kept_repair_cues(
            "I'll send it to you. No wait, tomorrow.",
            "I'll send it to you tomorrow."
        ));
        assert!(!model_kept_repair_cues(
            "Ask him about it. No wait, never mind.",
            "Ask him about it. Never mind."
        ));
        assert!(!model_kept_repair_cues(
            "I'll be late. Sorry, traffic is bad.",
            "Traffic is bad."
        ));
        assert!(!model_kept_repair_cues(
            "No wait, first. No wait, second.",
            "No wait, second."
        ));

        // Same or more occurrences -> true (case-insensitive).
        assert!(model_kept_repair_cues(
            "I'll send it to you. No wait, tomorrow.",
            "I'll send it to you. NO WAIT, tomorrow."
        ));
        assert!(model_kept_repair_cues(
            "We shipped it. Actually, the tests were green.",
            "We shipped it. actually, the tests were actually green."
        ));
        assert!(model_kept_repair_cues(
            "Send it to Devon.",
            "Send it to Devon."
        ));
        assert_eq!(
            repair_cue_count("Ask him about it. No wait, never mind."),
            2
        );
        assert_eq!(repair_cue_count("I'll send it to you tomorrow."), 0);
    }

    fn parse_jsonl_id_input(raw: &str) -> Vec<(String, String)> {
        raw.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|line| {
                let v: serde_json::Value = serde_json::from_str(line).unwrap();
                (
                    v["id"].as_str().unwrap().to_string(),
                    v["input"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    #[test]
    fn r1f_t4_english_filler_list_pinned_to_15_words() {
        assert_eq!(
            get_filler_words_for_language("en"),
            &[
                "uh", "um", "uhm", "umm", "uhh", "uhhh", "ah", "hmm", "hm", "mmm", "mm", "mh",
                "eh", "ehh", "ha",
            ]
        );
        // Dropping exempt English filler "hmm" passes; dropping any non-filler word fails.
        assert!(model_kept_words(
            "Send it to the client. No wait, hmm.",
            "Send it to the client. No wait."
        ));
        assert!(!model_kept_words(
            "Send it to the client. No wait, hmm.",
            "Send it to the. No wait, hmm."
        ));
        assert!(!model_kept_words(
            "I'll send it to you. No wait, tomorrow.",
            "I'll send it to. No wait, tomorrow."
        ));
        // Smart apostrophe, leading/trailing quote & underscore trimming, inner apostrophe, multiset count.
        assert!(model_kept_words(
            "I\u{2019}ll send 'tomorrow' _now_.",
            "i'll send tomorrow now"
        ));
        assert!(!model_kept_words("send send it", "send it"));
        assert_eq!(
            non_filler_word_count("Send it to the client. No wait, hmm."),
            7
        );
        assert_eq!(
            non_filler_word_count("Send it to the client. No wait."),
            7
        );
    }

    #[test]
    fn r1f_fresh_30_jsonl_parses_all_30_rows_valid_schema() {
        let raw = include_str!("../../../docs/eval/backtrack-fresh-r1f-30.jsonl");
        let mut seen = std::collections::HashSet::new();
        let mut count = 0usize;
        for (idx, line) in raw.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let line_no = idx + 1;
            let v: serde_json::Value = serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("line {line_no}: invalid json: {e}"));
            let id = v["id"].as_str().expect("id string");
            assert!(!id.is_empty(), "line {line_no}: empty id");
            assert!(seen.insert(id.to_string()), "line {line_no}: duplicate id {id}");
            let kind = v["kind"].as_str().expect("kind string");
            let input = v["input"].as_str().expect("input string");
            assert!(!input.is_empty(), "line {line_no}: empty input for {id}");
            let keep = v["keep"].as_array().expect("keep array");
            let drop = v["drop"].as_array().expect("drop array");
            match kind {
                "keep" => {
                    assert!(!keep.is_empty(), "line {line_no}: keep empty for {id}");
                    assert!(drop.is_empty(), "line {line_no}: drop non-empty for keep {id}");
                }
                "retract" => {
                    assert!(!keep.is_empty(), "line {line_no}: keep empty for {id}");
                    assert!(!drop.is_empty(), "line {line_no}: drop empty for retract {id}");
                }
                other => panic!("line {line_no}: unknown kind {other:?} for {id}"),
            }
            count += 1;
        }
        assert_eq!(count, 30);
    }

    #[test]
    fn r1f_t_rules_v11_guards_pairs_repros_and_95_adversarial_keeps() {
        let mut failures: Vec<String> = Vec::new();
        let check_exact = |failures: &mut Vec<String>, group: &str, id: &str, input: &str, expected: &str| {
            let got = rewrite_spoken_self_corrections(input);
            if got != expected {
                failures.push(format!(
                    "[{group}] {id}: expected={expected:?} got={got:?} input={input:?}"
                ));
            }
        };

        // Plan A v11 F3 table: exact "yes" take-backs, everything else in the pair/case files as said.
        let expected_yes: std::collections::HashMap<&str, &str> = std::collections::HashMap::from([
            ("b3y", "We need twelve chairs."),
            ("b3sy", "Dana baked four pies."),
            ("b5y", "meeting on wednesday"),
            ("b7y", "Meet on Wednesday."),
            ("b6y", "Call Ana."),
            ("r1a", "meeting on wednesday"),
            ("r1b", "Meeting on Wednesday."),
            ("r1c", "Meeting on Wednesday."),
            ("r1d", "Meeting on Wednesday."),
            ("c13", "Four pies."),
        ]);

        let pair_files: &[(&str, &str)] = &[
            ("pairs-1", include_str!("../../testdata/r1f/branch-evidence-pairs.jsonl")),
            ("pairs-2", include_str!("../../testdata/r1f/branch-evidence-pairs-2.jsonl")),
            ("pairs-3", include_str!("../../testdata/r1f/branch-evidence-pairs-3.jsonl")),
            ("pairs-4", include_str!("../../testdata/r1f/branch-evidence-pairs-4.jsonl")),
            ("pairs-5", include_str!("../../testdata/r1f/branch-evidence-pairs-5.jsonl")),
            ("pairs-6", include_str!("../../testdata/r1f/branch-evidence-pairs-6.jsonl")),
            ("rijul", include_str!("../../testdata/r1f/rijul-r1-clip4.jsonl")),
            ("repros", include_str!("../../testdata/r1f/cleo-531a-repros.jsonl")),
        ];

        for &(group, raw) in pair_files {
            for (id, input) in parse_jsonl_id_input(raw) {
                let expected = expected_yes
                    .get(id.as_str())
                    .copied()
                    .unwrap_or(input.as_str());
                check_exact(&mut failures, group, &id, &input, expected);
            }
        }

        // All 95 adversarial keeps (28 + 30 + 37) must come back byte-identical.
        let adv_files: &[(&str, &str, usize)] = &[
            ("adv-28", include_str!("../../testdata/r1f/adv/cleo-533a-keeps-28.jsonl"), 28),
            ("adv-30", include_str!("../../testdata/r1f/adv/brain-keeps-30.jsonl"), 30),
            ("adv-37", include_str!("../../testdata/r1f/adv/fresh-536-blind.jsonl"), 37),
        ];
        let mut adv_total = 0usize;
        for &(group, raw, expected_len) in adv_files {
            let rows = parse_jsonl_id_input(raw);
            assert_eq!(rows.len(), expected_len);
            adv_total += rows.len();
            for (id, input) in rows {
                check_exact(&mut failures, group, &id, &input, &input);
            }
        }
        assert_eq!(adv_total, 95);

        // AC-A9b cases: 18 take-backs stay retractions, ex01 and 4 known misses stay byte-identical.
        let a9b_rows = parse_jsonl_id_input(include_str!("../../testdata/r1f/a9b-cases.jsonl"));
        assert_eq!(a9b_rows.len(), 23);
        for (id, input) in a9b_rows {
            if id.starts_with("tb") {
                let got = rewrite_spoken_self_corrections(&input);
                if got == input {
                    failures.push(format!("[a9b] {id}: expected take-back, got unchanged {got:?}"));
                }
            } else {
                check_exact(&mut failures, "a9b", &id, &input, &input);
            }
        }

        // All 66 must-not-change sentences stay byte-identical.
        let mnc_rows = parse_jsonl_id_input(include_str!("../../../docs/eval/must-not-change-66.jsonl"));
        assert_eq!(mnc_rows.len(), 66);
        for (id, input) in mnc_rows {
            check_exact(&mut failures, "mnc-66", &id, &input, &input);
        }

        assert!(
            failures.is_empty(),
            "T-rules v11 failures ({}):\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn r1f_t_rules_guard_1_end_of_sentence() {
        assert_eq!(
            rewrite_spoken_self_corrections("Meet on Tuesday, sorry, Wednesday."),
            "Meet on Wednesday."
        );
        assert_eq!(
            rewrite_spoken_self_corrections("Call Sam, sorry, Ana, can you hear me?"),
            "Call Sam, sorry, Ana, can you hear me?"
        );
        assert_eq!(
            rewrite_spoken_self_corrections("Meet on Tuesday, sorry, Wednesday, if that works."),
            "Meet on Tuesday, sorry, Wednesday, if that works."
        );
    }

    #[test]
    fn r1f_t_rules_guard_2_question() {
        assert_eq!(
            rewrite_spoken_self_corrections("Ask Sam, no wait, Jordan?"),
            "Ask Sam, no wait, Jordan?"
        );
    }

    #[test]
    fn r1f_t_rules_guard_3_vocative() {
        assert_eq!(
            rewrite_spoken_self_corrections("Good job, Sam, sorry, Ana."),
            "Good job, Sam, sorry, Ana."
        );
    }

    #[test]
    fn r1f_t_rules_guard_4_first_word() {
        assert_eq!(
            rewrite_spoken_self_corrections("Sam, sorry, Ana."),
            "Sam, sorry, Ana."
        );
    }

    #[test]
    fn r1f_t_rules_guard_5_names_never_line_up() {
        assert_eq!(
            rewrite_spoken_self_corrections("Call Sam Lee, sorry, Ana Lee."),
            "Call Sam Lee, sorry, Ana Lee."
        );
    }

    #[test]
    fn r1f_t_rules_guard_6_three_words_or_fewer() {
        assert_eq!(
            rewrite_spoken_self_corrections(
                "The code is one two three four, I mean, one two three five."
            ),
            "The code is one two three four, I mean, one two three five."
        );
    }

    #[test]
    fn r1f_t_rules_guard_7_no_subject_word_start() {
        assert_eq!(
            rewrite_spoken_self_corrections("Tell me if it is five, sorry, it is six."),
            "Tell me if it is five, sorry, it is six."
        );
    }

    #[test]
    fn r1f_t_rules_guard_8_real_swap() {
        assert_eq!(
            rewrite_spoken_self_corrections("We need twelve chairs, sorry, twelve chairs."),
            "We need twelve chairs, sorry, twelve chairs."
        );
        assert_eq!(
            rewrite_spoken_self_corrections("Call Sam, sorry, Sam."),
            "Call Sam, sorry, Sam."
        );
    }

    #[test]
    fn r1f_t_rules_guard_9_interjection_list() {
        assert_eq!(
            rewrite_spoken_self_corrections("Call Priya, sorry, Wait."),
            "Call Priya, sorry, Wait."
        );
    }

    #[test]
    fn r1f_t_nopanic_rewrite_spoken_self_corrections() {
        // Copied verbatim from r1f-proto/rules-probe/src/main_v11_fuzz.rs (sha 287b3b9a957d).
        // No std::panic::set_hook in unit tests.
        const WORDS: &[&str] = &[
            "Sam", "Ana", "Priya", "Jordan", "Lee", "call", "meet", "on", "at", "the", "to", "a",
            "I", "we", "it", "you", "they", "my", "Tuesday", "wednesday", "May", "March", "one",
            "two", "twelve", "5", "4:50", "27th", "chairs", "pies", "boss", "wait", "thanks",
            "yes", "no", "never", "mind", "sorry", "mean", "actually", "rather", "scratch", "that",
            ",", ".", "?", "!", "'", "\u{2019}", "\u{2014}", "\u{00E9}", "\u{65E5}\u{672C}", "",
            "Sam,", "Ana?", "it's", "o'clock",
        ];
        const CUES: &[&str] = &[
            ", sorry, ", ", I mean ", ", sorry, I mean ", ", actually ", ", actually make it ",
            ", or rather, ", ", no wait, ", ", no, ", ", correction, ", ", let me rephrase, ",
            ", scratch that, ", ", never mind, ", ". Sorry, ", ". Scratch that. ",
            " scratch that ", " I mean ", " or rather ", ". No wait, ", ",sorry,", ", sorry,",
            " , scratch that , ",
        ];
        const EDGES: &[&str] = &[
            "", " ", ",", ".", ", sorry,", "sorry", ", sorry, ,", ". . .", "Scratch that.",
            ", scratch that, ,", "Sam, sorry,", ", sorry, Ana", "Sam. Sorry, Ana",
            "a, sorry, b, sorry, c, sorry, d",
        ];

        let mut inputs: Vec<String> = EDGES.iter().map(|e| (*e).to_string()).collect();
        let all_sets: &[&str] = &[
            include_str!("../../../docs/eval/must-not-change-66.jsonl"),
            include_str!("../../../docs/eval/backtrack-30.jsonl"),
            include_str!("../../../docs/eval/backtrack-fresh-20.jsonl"),
            include_str!("../../../docs/eval/backtrack-holdout-20.jsonl"),
            include_str!("../../../docs/eval/backtrack-fresh-r1f-30.jsonl"),
            include_str!("../../testdata/r1f/branch-evidence-pairs.jsonl"),
            include_str!("../../testdata/r1f/branch-evidence-pairs-2.jsonl"),
            include_str!("../../testdata/r1f/branch-evidence-pairs-3.jsonl"),
            include_str!("../../testdata/r1f/branch-evidence-pairs-4.jsonl"),
            include_str!("../../testdata/r1f/branch-evidence-pairs-5.jsonl"),
            include_str!("../../testdata/r1f/branch-evidence-pairs-6.jsonl"),
            include_str!("../../testdata/r1f/rijul-r1-clip4.jsonl"),
            include_str!("../../testdata/r1f/t6-v7.jsonl"),
            include_str!("../../testdata/r1f/cleo-531a-repros.jsonl"),
            include_str!("../../testdata/r1f/a9b-cases.jsonl"),
            include_str!("../../testdata/r1f/adv/cleo-533a-keeps-28.jsonl"),
            include_str!("../../testdata/r1f/adv/brain-keeps-30.jsonl"),
            include_str!("../../testdata/r1f/adv/fresh-536-blind.jsonl"),
        ];
        for raw in all_sets {
            for (_, inp) in parse_jsonl_id_input(raw) {
                inputs.push(inp);
            }
        }

        let mut s: u64 = 20261120;
        let mut next = |m: usize| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s % m as u64) as usize
        };
        for _ in 0..1000 {
            let mut t = String::new();
            let cues_at: Vec<usize> = (0..1 + next(3)).map(|_| next(15)).collect();
            let words = next(15);
            for i in 0..15 {
                if cues_at.contains(&i) {
                    t.push_str(CUES[next(CUES.len())]);
                }
                if i < words {
                    t.push_str(WORDS[next(WORDS.len())]);
                    t.push_str([" ", "", "  "][next(3)]);
                }
            }
            inputs.push(t);
        }

        let bad: Vec<&String> = inputs
            .iter()
            .filter(|t| std::panic::catch_unwind(|| rewrite_spoken_self_corrections(t)).is_err())
            .collect();
        assert!(
            bad.is_empty(),
            "T-nopanic: {} of {} inputs panicked: {:?}",
            bad.len(),
            inputs.len(),
            bad
        );
    }

    #[test]
    fn f_a1_extract_punctuation_multibyte_utf8_safe() {
        let sam = vec!["Sam".to_string()];
        let jp = vec!["日本".to_string()];
        let cases: &[(&str, &[String], &str)] = &[
            ("\u{201C}Sam said hi", &sam, "\u{201C}Sam said hi"),
            ("hi Sam\u{201D}", &sam, "hi Sam\u{201D}"),
            ("\u{2014}Sam", &sam, "\u{2014}Sam"),
            ("Sam\u{2026}", &sam, "Sam\u{2026}"),
            ("\u{00AB}Sam\u{00BB}", &sam, "\u{00AB}Sam\u{00BB}"),
            ("日本，", &jp, "日本，"),
            ("\u{201C}sam said hi", &sam, "\u{201C}Sam said hi"),
            ("\u{2014}sam", &sam, "\u{2014}Sam"),
            ("\u{00AB}sam\u{00BB}", &sam, "\u{00AB}Sam\u{00BB}"),
        ];
        let mut panicked: Vec<&str> = Vec::new();
        for &(input, words, expected) in cases {
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                apply_custom_words(input, words, 0.18)
            }));
            match res {
                Ok(out) => assert_eq!(out, expected, "input={input:?}"),
                Err(_) => panicked.push(input),
            }
        }
        assert!(
            panicked.is_empty(),
            "F-a1 panicked on {}/{} cases: {:?}",
            panicked.len(),
            cases.len(),
            panicked
        );
    }

    #[test]
    fn f_a2_wide_sweep_all_7_text_entry_points_never_panic() {
        // Copied from r1f-proto/rules-probe/src/main_wide_sweep.rs (sha e96a76ae168d), 2,000 strings.
        // No std::panic::set_hook in unit tests.
        const W: &[&str] = &[
            "Sam", "sam", "Kubernetes", "kubernetes", "the", "to", "um", "uh", "I", "it's",
            "sorry,", "scratch", "that", "\u{201C}Sam", "Sam\u{201D}", "\u{2014}", "\u{2026}",
            "\u{AB}Sam\u{BB}", "é", "日本", "Ñandú", "'", "\u{2019}", ",", ".", "",
            "Kuber\u{2011}netes", "\u{1F600}", "Sam\u{1F600}", "\u{1F600}Sam", "naïve", "café,",
            "\u{FEFF}",
        ];
        let mut s: u64 = 0x9E3779B97F4A7C15;
        let mut next = |m: usize| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s % m as u64) as usize
        };
        let words = vec![
            "Sam".to_string(),
            "Kubernetes".to_string(),
            "café".to_string(),
            "日本".to_string(),
        ];
        let corr = vec![
            crate::settings::CorrectionPair {
                from: "kuber".into(),
                to: "Kubernetes".into(),
            },
            crate::settings::CorrectionPair {
                from: "日本".into(),
                to: "Japan".into(),
            },
        ];
        let blocks = vec![crate::settings::SavedBlock {
            id: "1".into(),
            cue: "sig".into(),
            content: "Best, Sam".into(),
        }];

        let mut bad: std::collections::BTreeMap<&str, (usize, String)> = Default::default();
        let n = 2000usize;
        for _ in 0..n {
            let mut a = String::new();
            for _ in 0..next(12) {
                a.push_str(W[next(W.len())]);
                a.push_str([" ", "", "  "][next(3)]);
            }
            let mut b = String::new();
            for _ in 0..next(12) {
                b.push_str(W[next(W.len())]);
                b.push(' ');
            }

            let mut record = |name: &'static str, ok: bool| {
                if !ok {
                    let e = bad
                        .entry(name)
                        .or_insert_with(|| (0, format!("{a:?} | {b:?}")));
                    e.0 += 1;
                }
            };

            record(
                "apply_custom_words",
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    apply_custom_words(&a, &words, 0.18);
                }))
                .is_ok(),
            );
            record(
                "apply_corrections",
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    apply_corrections(&a, &corr);
                }))
                .is_ok(),
            );
            record(
                "suggest_spelling_corrections",
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    suggest_spelling_corrections(&a, &b, &words, &corr);
                }))
                .is_ok(),
            );
            record(
                "expand_saved_blocks",
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    expand_saved_blocks(&a, &blocks);
                }))
                .is_ok(),
            );
            record(
                "filter_transcription_output",
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    filter_transcription_output(&a, "en", &None);
                }))
                .is_ok(),
            );
            record(
                "remember_spelling_correction",
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut st = crate::settings::get_default_settings();
                    remember_spelling_correction(&mut st, &a, &b);
                }))
                .is_ok(),
            );
            record(
                "rewrite_spoken_self_corrections",
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    rewrite_spoken_self_corrections(&a);
                }))
                .is_ok(),
            );
        }

        assert!(
            bad.is_empty(),
            "F-a2 wide sweep ({n} x 7) panicked: {:?}",
            bad
        );
    }

    // Verbatim copy of X3l `extract_punctuation` (041643da74e8, text.rs:233-253)
    // used only to verify ASCII byte-identity for F-a1b.
    fn extract_punctuation_x3l(word: &str) -> (&str, &str) {
        let prefix_end = word.chars().take_while(|c| !c.is_alphanumeric()).count();
        let suffix_start = word
            .char_indices()
            .rev()
            .take_while(|(_, c)| !c.is_alphanumeric())
            .count();

        let prefix = if prefix_end > 0 {
            &word[..prefix_end]
        } else {
            ""
        };

        let suffix = if suffix_start > 0 {
            &word[word.len() - suffix_start..]
        } else {
            ""
        };

        (prefix, suffix)
    }

    #[test]
    fn f_a1b_extract_punctuation_matches_x3l_on_all_ascii_tokens() {
        let explicit_punct = ["?", "-", "...", ",", "--", "!?", ".", "'"];
        let mut explicit_diffs: Vec<String> = Vec::new();
        for tok in explicit_punct {
            let got = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                extract_punctuation(tok)
            }))
            .expect("extract_punctuation panicked on explicit ASCII token");
            let want = extract_punctuation_x3l(tok);
            if got != want {
                explicit_diffs.push(format!("{tok:?}: got={got:?} want={want:?}"));
            }
        }

        // Exact F-a2 2,000-string sweep generator.
        const W: &[&str] = &[
            "Sam", "sam", "Kubernetes", "kubernetes", "the", "to", "um", "uh", "I", "it's",
            "sorry,", "scratch", "that", "\u{201C}Sam", "Sam\u{201D}", "\u{2014}", "\u{2026}",
            "\u{AB}Sam\u{BB}", "é", "日本", "Ñandú", "'", "\u{2019}", ",", ".", "",
            "Kuber\u{2011}netes", "\u{1F600}", "Sam\u{1F600}", "\u{1F600}Sam", "naïve", "café,",
            "\u{FEFF}",
        ];
        let mut s: u64 = 0x9E3779B97F4A7C15;
        let mut next = |m: usize| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s % m as u64) as usize
        };

        let mut checked_tokens = 0usize;
        let mut diff_tokens = 0usize;
        let mut diff_strings = 0usize;
        let mut sample_diffs: Vec<String> = Vec::new();

        for _ in 0..2000usize {
            let mut a = String::new();
            for _ in 0..next(12) {
                a.push_str(W[next(W.len())]);
                a.push_str([" ", "", "  "][next(3)]);
            }
            let mut b = String::new();
            for _ in 0..next(12) {
                b.push_str(W[next(W.len())]);
                b.push(' ');
            }
            let _ = &b;

            let mut string_had_diff = false;
            for tok in a.split_whitespace().filter(|t| t.is_ascii()) {
                checked_tokens += 1;
                let got = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    extract_punctuation(tok)
                }))
                .expect("extract_punctuation panicked on sweep ASCII token");
                let want = extract_punctuation_x3l(tok);
                if got != want {
                    diff_tokens += 1;
                    string_had_diff = true;
                    if sample_diffs.len() < 10 {
                        sample_diffs.push(format!("{tok:?}: got={got:?} want={want:?}"));
                    }
                }
            }
            if string_had_diff {
                diff_strings += 1;
            }
        }

        assert!(checked_tokens > 0, "expected ASCII tokens in F-a2 sweep");
        assert!(
            explicit_diffs.is_empty() && diff_tokens == 0,
            "F-a1b ASCII identity mismatch: explicit={}/{} ({:?}), sweep_tokens={}/{} across {} strings ({:?})",
            explicit_diffs.len(),
            explicit_punct.len(),
            explicit_diffs,
            diff_tokens,
            checked_tokens,
            diff_strings,
            sample_diffs
        );
    }

    #[test]
    fn f_a1b_apply_custom_words_ascii_punctuation_tokens_end_to_end() {
        let words = vec![
            "Sam".to_string(),
            "Kubernetes".to_string(),
            "café".to_string(),
            "日本".to_string(),
        ];
        let cases: &[(&str, &str)] = &[
            ("Is that you Sam ?", "Is that you Sam?"),
            ("Sam ?", "Sam?"),
            ("Sam -", "Sam-"),
            ("Sam ...", "Sam..."),
            ("Sam , Ana", "Sam, Ana"),
            ("Call Sam - then Ana", "Call Sam- then Ana"),
        ];

        let mut failures: Vec<String> = Vec::new();
        for &(input, expected) in cases {
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                apply_custom_words(input, &words, 0.18)
            }));
            match res {
                Ok(out) if out == expected => {}
                Ok(out) => failures.push(format!("{input:?}: got={out:?} expected={expected:?}")),
                Err(_) => failures.push(format!("{input:?}: PANICKED")),
            }
        }

        assert!(
            failures.is_empty(),
            "F-a1b ASCII end-to-end failures ({}/{}):\n{}",
            failures.len(),
            cases.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn f_a1b_apply_custom_words_ellipsis_no_panic() {
        let words = vec![
            "Sam".to_string(),
            "Kubernetes".to_string(),
            "café".to_string(),
            "日本".to_string(),
        ];
        let cases: &[(&str, &str)] = &[
            ("\u{2026}", "\u{2026}"),
            ("Sam \u{2026}", "Sam\u{2026}"),
            ("\u{201C}Sam said hi", "\u{201C}Sam said hi"),
        ];

        let mut failures: Vec<String> = Vec::new();
        for &(input, expected) in cases {
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                apply_custom_words(input, &words, 0.18)
            }));
            match res {
                Ok(out) if out == expected => {}
                Ok(out) => failures.push(format!("{input:?}: got={out:?} expected={expected:?}")),
                Err(_) => failures.push(format!("{input:?}: PANICKED")),
            }
        }

        assert!(
            failures.is_empty(),
            "F-a1b ellipsis no-panic failures ({}/{}):\n{}",
            failures.len(),
            cases.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn f1c_t1_determiner_before_target_is_consumed_when_added_has_determiner() {
        let cases: &[(&str, &str)] = &[
            (
                "Pack the gloves, the scarf, and the whistle, scratch the whistle, add the compass.",
                "Pack the gloves, the scarf, and the compass.",
            ),
            (
                "Bring the tent and the stove, scratch the stove, add a lantern.",
                "Bring the tent and a lantern.",
            ),
            (
                "Order the red chairs, scratch the red chairs, add the blue chairs.",
                "Order the blue chairs.",
            ),
            (
                "Pack her whistle, scratch the whistle, add the compass.",
                "Pack the compass.",
            ),
            (
                "Email that report, scratch the report, add the slides.",
                "Email the slides.",
            ),
        ];

        let mut failures: Vec<String> = Vec::new();
        for &(input, expected) in cases {
            let got = rewrite_spoken_self_corrections(input);
            if got != expected {
                failures.push(format!("{input:?}: got={got:?} expected={expected:?}"));
            }
        }
        assert!(
            failures.is_empty(),
            "F1c-T1 failures ({}/{}):\n{}",
            failures.len(),
            cases.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn f1c_t2_unchanged_rows_and_empty_target_guard() {
        let cases: &[(&str, &str)] = &[
            (
                "Add milk, eggs and butter, scratch the butter, add cheese.",
                "Add milk, eggs and cheese.",
            ),
            (
                "Pack gloves and whistle, scratch the whistle, add the compass.",
                "Pack gloves and the compass.",
            ),
            (
                "Pack the whistle, scratch the  , add the compass.",
                "Pack the whistle, scratch the  , add the compass.",
            ),
        ];

        let mut failures: Vec<String> = Vec::new();
        for &(input, expected) in cases {
            let got = rewrite_spoken_self_corrections(input);
            if got != expected {
                failures.push(format!("{input:?}: got={got:?} expected={expected:?}"));
            }
        }
        assert!(
            failures.is_empty(),
            "F1c-T2 failures ({}/{}):\n{}",
            failures.len(),
            cases.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn f1c_t3_no_new_doubled_words_across_eval_sets_and_scratch_that_unchanged() {
        let doubled_words = |text: &str| -> std::collections::HashSet<String> {
            let words: Vec<String> = text
                .split_whitespace()
                .map(norm_repair_token)
                .filter(|w| !w.is_empty())
                .collect();
            words
                .windows(2)
                .filter(|pair| pair[0] == pair[1])
                .map(|pair| pair[0].clone())
                .collect()
        };

        let sets: &[(&str, &str)] = &[
            (
                "must-not-change-66",
                include_str!("../../../docs/eval/must-not-change-66.jsonl"),
            ),
            (
                "backtrack-30",
                include_str!("../../../docs/eval/backtrack-30.jsonl"),
            ),
            (
                "backtrack-fresh-20",
                include_str!("../../../docs/eval/backtrack-fresh-20.jsonl"),
            ),
            (
                "backtrack-holdout-20",
                include_str!("../../../docs/eval/backtrack-holdout-20.jsonl"),
            ),
            (
                "backtrack-fresh-r1f-30",
                include_str!("../../../docs/eval/backtrack-fresh-r1f-30.jsonl"),
            ),
            (
                "a9b-cases",
                include_str!("../../testdata/r1f/a9b-cases.jsonl"),
            ),
        ];

        let mut total_rows = 0usize;
        let mut doubled_failures: Vec<String> = Vec::new();
        for &(set_name, raw) in sets {
            for line in raw.lines().filter(|l| !l.trim().is_empty()) {
                let v: serde_json::Value = serde_json::from_str(line).unwrap();
                let id = v["id"].as_str().unwrap_or("?");
                let input = v["input"].as_str().unwrap();
                total_rows += 1;
                let out = rewrite_spoken_self_corrections(input);
                let in_doubles = doubled_words(input);
                let out_doubles = doubled_words(&out);
                let added_doubles: Vec<&String> = out_doubles.difference(&in_doubles).collect();
                if !added_doubles.is_empty() {
                    doubled_failures.push(format!(
                        "{set_name}:{id}: added_doubles={added_doubles:?} out={out:?}"
                    ));
                }
            }
        }
        assert_eq!(total_rows, 189, "expected 166 T5 rows + 23 a9b cases");

        let scratch_that_cases: &[(&str, &str)] = &[
            ("Send it to Priya, scratch that, Devon.", "Send it to Devon."),
            (
                "The meeting is Tuesday, scratch that, Wednesday.",
                "The meeting is Wednesday.",
            ),
            (
                "Book that room, scratch that, book the other room.",
                "Book that room, scratch that, book the other room.",
            ),
            ("Scratch that off the list.", "Scratch that off the list."),
            (
                "Add milk and that cheese, scratch that, add bread.",
                "Add milk and that cheese, scratch that, add bread.",
            ),
            (
                "Email that report, scratch the report, add the slides.",
                "Email the slides.",
            ),
        ];
        let mut that_failures: Vec<String> = Vec::new();
        for &(input, expected) in scratch_that_cases {
            let got = rewrite_spoken_self_corrections(input);
            if got != expected {
                that_failures.push(format!("{input:?}: got={got:?} expected={expected:?}"));
            }
        }

        assert!(
            doubled_failures.is_empty() && that_failures.is_empty(),
            "F1c-T3 failures: doubled={}/{} ({:?}), scratch_that={}/{} ({:?})",
            doubled_failures.len(),
            total_rows,
            doubled_failures,
            that_failures.len(),
            scratch_that_cases.len(),
            that_failures
        );
    }

    #[test]
    fn f1c_t4_multiple_target_occurrences_replace_same_occurrence() {
        let cases: &[(&str, &str)] = &[
            (
                "Pack the whistle, the rope and a whistle, scratch the whistle, add the compass.",
                "Pack the compass, the rope and a whistle.",
            ),
            (
                "Pack a whistle, the rope and the whistle, scratch the whistle, add a compass.",
                "Pack a compass, the rope and the whistle.",
            ),
            (
                "Pack whistle, the rope and the whistle, scratch the whistle, add the compass.",
                "Pack the compass, the rope and the whistle.",
            ),
            (
                "Bring the red chairs and the red chairs, scratch the red chairs, add the blue chairs.",
                "Bring the blue chairs and the red chairs.",
            ),
        ];

        let mut failures: Vec<String> = Vec::new();
        for &(input, expected) in cases {
            let got = rewrite_spoken_self_corrections(input);
            if got != expected {
                failures.push(format!("{input:?}: got={got:?} expected={expected:?}"));
            }
        }
        assert!(
            failures.is_empty(),
            "F1c-T4 failures ({}/{}):\n{}",
            failures.len(),
            cases.len(),
            failures.join("\n")
        );
    }
}
