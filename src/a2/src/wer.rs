//! Word error rate: Levenshtein edit distance + corpus aggregation.
//!
//! Per-utterance `(errors, ref_len)` pairs feed `eval` bootstrap CIs
//! (`corpus_delta_ci` for 1-best vs oracle deltas). Corpus WER is
//! `sum(errors)/sum(ref_len)`; utterances with empty refs contribute
//! `hyp.len()` errors over a zero denominator and are reported separately.

/// Edit distance between word sequences (sub/ins/del = 1).
pub fn edit_distance(reference: &[String], hypothesis: &[String]) -> u32 {
    let n = reference.len();
    let m = hypothesis.len();
    if n == 0 {
        return m as u32;
    }
    if m == 0 {
        return n as u32;
    }
    let mut prev: Vec<u32> = (0..=m as u32).collect();
    let mut curr = vec![0u32; m + 1];
    for (i, rw) in reference.iter().enumerate() {
        curr[0] = i as u32 + 1;
        for (j, hw) in hypothesis.iter().enumerate() {
            let sub = prev[j] + u32::from(rw != hw);
            let ins = curr[j] + 1;
            let del = prev[j + 1] + 1;
            curr[j + 1] = sub.min(ins).min(del);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[m]
}

/// Corpus WER over per-utterance `(errors, ref_len)` pairs.
pub fn wer_corpus(pairs: &[(u32, u32)]) -> f64 {
    let e: u64 = pairs.iter().map(|&(e, _)| e as u64).sum();
    let d: u64 = pairs.iter().map(|&(_, d)| d as u64).sum();
    if d == 0 { 0.0 } else { e as f64 / d as f64 }
}

/// Corpus summary with plausibility counts.
#[derive(Clone, Debug)]
pub struct WerSummary {
    pub utterances: usize,
    pub ref_words: u64,
    pub errors: u64,
    pub wer: f64,
}

impl WerSummary {
    pub fn from_pairs(pairs: &[(u32, u32)]) -> Self {
        let ref_words: u64 = pairs.iter().map(|&(_, d)| d as u64).sum();
        let errors: u64 = pairs.iter().map(|&(e, _)| e as u64).sum();
        WerSummary {
            utterances: pairs.len(),
            ref_words,
            errors,
            wer: wer_corpus(pairs),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn identical_is_zero() {
        assert_eq!(edit_distance(&w(&["a", "b"]), &w(&["a", "b"])), 0);
    }

    #[test]
    fn substitution_counts_one() {
        assert_eq!(edit_distance(&w(&["a", "b"]), &w(&["a", "c"])), 1);
    }

    #[test]
    fn insertion_and_deletion() {
        assert_eq!(edit_distance(&w(&["a"]), &w(&["a", "b"])), 1);
        assert_eq!(edit_distance(&w(&["a", "b"]), &w(&["a"])), 1);
    }

    #[test]
    fn empty_hyp_is_all_deletions() {
        assert_eq!(edit_distance(&w(&["a", "b", "c"]), &w(&[])), 3);
    }

    #[test]
    fn corpus_wer_sums() {
        assert!((wer_corpus(&[(1, 4), (1, 4)]) - 0.25).abs() < 1e-12);
        assert_eq!(wer_corpus(&[]), 0.0);
    }
}
