//! Candidate scoring for the automatic tagger.
//!
//! A faithful port of Harmony's unified scorer, extended with four more
//! signals: title, artist, duration, genre, label, bpm and key each contribute
//! a similarity in `0.0..=1.0`, combined into a weighted sum. Text similarity
//! normalizes punctuation and case, then compares word by word with a
//! hand-rolled Levenshtein metric — no new dependency.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::error::{CrateError, Result};
use crate::models::ScoredTagCandidate;

/// Relative weights of the seven similarity signals (must sum to `1.0`).
///
/// The struct doubles as the JSON shape persisted in
/// `AppSettings::tagger_weights`, so field names stay snake_case on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(crate) struct ScoringWeights {
    pub title: f64,
    pub artist: f64,
    pub duration: f64,
    pub genre: f64,
    pub label: f64,
    pub bpm: f64,
    pub key: f64,
}

/// Default weights: mostly title, then artist, with duration, genre, label,
/// bpm and key as secondary signals. They sum to exactly `1.0`.
pub(crate) const DEFAULT_WEIGHTS: ScoringWeights = ScoringWeights {
    title: 0.40,
    artist: 0.25,
    duration: 0.10,
    genre: 0.10,
    label: 0.08,
    bpm: 0.04,
    key: 0.03,
};

/// Two scores closer than this are treated as tied and broken by provider order.
///
/// The boundary is the computed f64 `leader - member <= 0.01` with deliberately
/// **no epsilon**: an epsilon would only move the knife edge and add a magic
/// constant, and an exact `0.01` mathematical gap is a measure-zero event for
/// Levenshtein-derived scores. Both sides are pinned by
/// `rank_candidates_grouping_boundary_follows_the_computed_gap`.
const TIE_TOLERANCE: f64 = 0.01;

/// Lowercase `s`, turn every character that is not alphanumeric into a word
/// separator, collapse whitespace runs and trim.
///
/// Unlike Harmony's ASCII-only `[^\w\s]`, any Unicode alphanumeric character
/// survives, so accented and non-Latin titles/artists are not erased.
pub(crate) fn normalize_string(s: &str) -> String {
    let separated: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    separated
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// `0.0..=1.0` similarity derived from the Levenshtein distance over `char`s.
///
/// `1.0` for equal strings, `0.0` when either side is empty, otherwise
/// `1 - distance / max(len(a), len(b))`.
pub(crate) fn levenshtein_similarity(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let distance = levenshtein_distance(a, b);
    let longest = a.chars().count().max(b.chars().count());
    1.0 - (distance as f64 / longest as f64)
}

/// Classic Levenshtein distance with a two-row rolling buffer, counting `char`s.
fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }

    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0usize; b.len() + 1];

    for (i, a_char) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, b_char) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(a_char != b_char);
            let deletion = previous[j + 1] + 1;
            let insertion = current[j] + 1;
            current[j + 1] = substitution.min(deletion).min(insertion);
        }
        std::mem::swap(&mut previous, &mut current);
    }

    previous[b.len()]
}

/// Genre similarity: [`hybrid_text_similarity`] over the genre text.
///
/// Empty against non-empty scores `0.0`; empty against empty scores `1.0`
/// (both unknown — no penalty either way).
pub(crate) fn genre_score(local: &str, candidate: &str) -> f64 {
    hybrid_text_similarity(local, candidate)
}

/// Label similarity, same rule as [`genre_score`].
pub(crate) fn label_score(local: &str, candidate: &str) -> f64 {
    hybrid_text_similarity(local, candidate)
}

/// BPM similarity bucketed like [`duration_score`]: `±1 -> 1.0`, `±3 -> 0.8`,
/// `±5 -> 0.5`, larger gaps `-> 0.2`. A zero local or candidate BPM is neutral
/// (`0.7`), so a provider that omits BPM neither rewards nor punishes.
pub(crate) fn bpm_score(local: f64, candidate: Option<f64>) -> f64 {
    let candidate_bpm = match candidate {
        Some(candidate) if candidate != 0.0 => candidate,
        _ => return 0.7,
    };
    if local == 0.0 {
        return 0.7;
    }

    let diff = (local - candidate_bpm).abs();
    if diff <= 1.0 {
        1.0
    } else if diff <= 3.0 {
        0.8
    } else if diff <= 5.0 {
        0.5
    } else {
        0.2
    }
}

/// Musical-key match: exact equality scores `1.0`, two known but different keys
/// score `0.2`, and an empty local or candidate key is neutral (`0.5`). The
/// comparison is literal — Camelot short notation and standard names are not
/// cross-converted here — so callers should feed consistent key text.
pub(crate) fn key_score(local: &str, candidate: &str) -> f64 {
    if local.is_empty() || candidate.is_empty() {
        0.5
    } else if local == candidate {
        1.0
    } else {
        0.2
    }
}

/// Asymmetric word-level similarity between two free-text strings.
///
/// Both sides are normalized; equality short-circuits to `1.0`. For each query
/// word the best Levenshtein similarity against every candidate word is taken,
/// and the mean of those maxima is returned. Extra candidate words therefore do
/// not penalize a shorter query, and reordering still scores `1.0`.
pub(crate) fn hybrid_text_similarity(query: &str, candidate: &str) -> f64 {
    let query = normalize_string(query);
    let candidate = normalize_string(candidate);
    if query == candidate {
        return 1.0;
    }

    let query_words: Vec<&str> = query.split_whitespace().collect();
    let candidate_words: Vec<&str> = candidate.split_whitespace().collect();
    if query_words.is_empty() || candidate_words.is_empty() {
        return 0.0;
    }

    let total: f64 = query_words
        .iter()
        .map(|query_word| {
            candidate_words
                .iter()
                .map(|candidate_word| levenshtein_similarity(query_word, candidate_word))
                .fold(0.0_f64, f64::max)
        })
        .sum();

    total / query_words.len() as f64
}

/// Harmony's `calculateDurationScore`, with the thresholds expressed in
/// milliseconds (`5_000 / 15_000 / 30_000`).
///
/// A missing or zero candidate duration, or a zero local duration, is neutral
/// (`0.7`). Otherwise the absolute difference is bucketed into `1.0 / 0.8 / 0.5 / 0.2`.
/// The comparison is total for any `i64` pair, so extreme durations cannot panic.
pub(crate) fn duration_score(local_ms: i64, candidate_ms: Option<i64>) -> f64 {
    let candidate_ms = match candidate_ms {
        Some(candidate) if candidate != 0 => candidate,
        _ => return 0.7,
    };
    if local_ms == 0 {
        return 0.7;
    }

    // `abs_diff` is total: a plain `(local_ms - candidate_ms).abs()` overflows on
    // extreme untrusted durations (panic with overflow checks, wrap in release).
    let diff = local_ms.abs_diff(candidate_ms);
    if diff <= 5_000 {
        1.0
    } else if diff <= 15_000 {
        0.8
    } else if diff <= 30_000 {
        0.5
    } else {
        0.2
    }
}

/// The weighted scorer over title / artist / duration / genre / label / bpm / key.
pub(crate) struct UnifiedScorer {
    weights: ScoringWeights,
}

impl UnifiedScorer {
    /// Build a scorer, rejecting weights whose sum is off by more than `0.001`.
    ///
    /// Production constructs the scorer here when the user configured custom
    /// weights through `AppSettings::tagger_weights`; [`Self::default_weights`]
    /// remains the infallible path for absent or invalid configurations.
    pub(crate) fn new(weights: ScoringWeights) -> Result<Self> {
        let sum = weights.title
            + weights.artist
            + weights.duration
            + weights.genre
            + weights.label
            + weights.bpm
            + weights.key;
        if (sum - 1.0).abs() > 0.001 {
            return Err(CrateError::Tagger(format!(
                "scoring weights must sum to 1.0, got {sum}"
            )));
        }
        Ok(Self { weights })
    }

    /// The infallible default scorer (title 0.40, artist 0.25, duration 0.10,
    /// genre 0.10, label 0.08, bpm 0.04, key 0.03).
    pub(crate) fn default_weights() -> Self {
        Self {
            weights: DEFAULT_WEIGHTS,
        }
    }

    /// Weighted sum of the title, artist, duration, genre, label, bpm and key
    /// similarities.
    // The IPC-driven flat signature is deliberate: callers stay positional and
    // the weights stay the single configuration surface.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn score(
        &self,
        local_title: &str,
        local_artist: &str,
        local_duration_ms: i64,
        local_genre: &str,
        local_label: &str,
        local_bpm: f64,
        local_key: &str,
        candidate_title: &str,
        candidate_artist: &str,
        candidate_duration_ms: Option<i64>,
        candidate_genre: Option<&str>,
        candidate_label: Option<&str>,
        candidate_bpm: Option<f64>,
        candidate_key: Option<&str>,
    ) -> f64 {
        hybrid_text_similarity(local_title, candidate_title) * self.weights.title
            + hybrid_text_similarity(local_artist, candidate_artist) * self.weights.artist
            + duration_score(local_duration_ms, candidate_duration_ms) * self.weights.duration
            + genre_score(local_genre, candidate_genre.unwrap_or("")) * self.weights.genre
            + label_score(local_label, candidate_label.unwrap_or("")) * self.weights.label
            + bpm_score(local_bpm, candidate_bpm) * self.weights.bpm
            + key_score(local_key, candidate_key.unwrap_or("")) * self.weights.key
    }
}

/// Filter `>= min_score`, sort descending, then order each maximal near-tie run
/// (members within [`TIE_TOLERANCE`] of the run's leader) by `provider_priority`
/// index, and finally truncate to `max_candidates`.
///
/// The provider tie-break deliberately does **not** live in the `sort_by`
/// comparator: a `<= TIE_TOLERANCE` comparison is not transitive (scores
/// `0.99 / 0.995 / 1.0` give `0.99 < 0.995` and `0.995 < 1.0` by tolerance, but
/// `1.0 < 0.99` because `1.0 - 0.99 = 0.010000000000000009 > 0.01` — a cycle),
/// and Rust requires a total order. Sorting by score first is a total order; the
/// stable priority pass then yields a deterministic near-tie ordering. Candidates
/// equal on both score and provider index keep their input (provider search) order.
///
/// The returned order is a **rank, not a score sort**: inside one near-tie group
/// the higher-priority provider can place a slightly lower `similarity_score`
/// first, so callers must not assume `similarity_score` is non-increasing.
pub(crate) fn rank_candidates(
    mut candidates: Vec<ScoredTagCandidate>,
    provider_priority: &[String],
    min_score: f64,
    max_candidates: usize,
) -> Vec<ScoredTagCandidate> {
    let priority_index = |provider: &str| -> usize {
        provider_priority
            .iter()
            .position(|priority| priority == provider)
            .unwrap_or(usize::MAX)
    };

    candidates.retain(|candidate| candidate.similarity_score >= min_score);
    candidates.sort_by(|a, b| {
        b.similarity_score
            .partial_cmp(&a.similarity_score)
            .unwrap_or(Ordering::Equal)
    });

    // A tolerance-based comparator is not transitive, so the provider tie-break
    // cannot live in `sort_by`. Sort by score first (a total order), then order
    // each maximal run whose members are within TIE_TOLERANCE of the run's leader
    // by provider priority. Stable, so equal candidates keep their search order.
    let mut group_start = 0;
    while group_start < candidates.len() {
        let leader = candidates[group_start].similarity_score;
        let mut group_end = group_start + 1;
        while group_end < candidates.len()
            && leader - candidates[group_end].similarity_score <= TIE_TOLERANCE
        {
            group_end += 1;
        }
        candidates[group_start..group_end]
            .sort_by_key(|candidate| priority_index(&candidate.candidate.provider));
        group_start = group_end;
    }

    candidates.truncate(max_candidates);
    candidates
}
