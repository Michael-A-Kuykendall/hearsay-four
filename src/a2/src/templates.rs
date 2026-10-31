//! Word templates + SIL model over the frozen A1 front end.
//!
//! `load_templates` reads any `--template-dir` with a run1-shaped
//! `manifest.tsv` (`file<TAB>label<TAB>...`, header row, `enroll.wav` skipped).
//! Word models use TRIMMED frames (speech only); the SIL mean comes from
//! UNTRIMMED lead/trail frames (recording silence). Swap path for L1: point
//! `--template-dir` at the canonical output, zero code change.

use fse::frontend::{FrontendConfig, extract_untrimmed_with_config, extract_with_config};
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

/// One isolated-word template instance (a single take).
#[derive(Clone, Debug)]
pub struct WordTemplate {
    pub word: String,
    pub take: usize,
    pub frames: Vec<Vec<f64>>,
    /// Full-clip samples (for concat phrase synthesis + SIL stats).
    pub samples: Vec<i16>,
    pub sample_rate: u32,
}

/// Decodable template set: word instances + SIL mean + indexes.
#[derive(Clone, Debug)]
pub struct TemplateSet {
    pub templates: Vec<WordTemplate>,
    pub words: Vec<String>,
    pub word_to_instances: HashMap<String, Vec<usize>>,
    /// Mean silence frame from lead/trail of all clips (dim = cfg.dim()).
    pub sil_mean: Vec<f64>,
    /// Lead/trail frames pooled per clip (for stats/debugging).
    pub sil_pool_frames: usize,
    pub frame_shift_ms: f64,
    pub sample_rate: u32,
}

impl TemplateSet {
    /// Instances of `word` (indexes into `templates`).
    pub fn instances_of(&self, word: &str) -> &[usize] {
        self.word_to_instances
            .get(word)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Synthetic set for unit tests (no audio): one take per word.
    pub fn synthetic(words: Vec<(&str, Vec<Vec<f64>>)>, sil_mean: Vec<f64>) -> TemplateSet {
        let templates: Vec<WordTemplate> = words
            .into_iter()
            .map(|(w, frames)| WordTemplate {
                word: w.to_string(),
                take: 0,
                frames,
                samples: Vec::new(),
                sample_rate: 16_000,
            })
            .collect();
        Self::index(templates, sil_mean, 0)
    }

    /// View excluding one take (cross-take DEV: templates take != held-out).
    pub fn excluding_take(&self, heldout: usize) -> TemplateSet {
        let templates: Vec<WordTemplate> = self
            .templates
            .iter()
            .filter(|t| t.take != heldout)
            .cloned()
            .collect();
        Self::index(templates, self.sil_mean.clone(), self.sil_pool_frames)
    }

    fn index(
        templates: Vec<WordTemplate>,
        sil_mean: Vec<f64>,
        sil_pool_frames: usize,
    ) -> TemplateSet {
        let mut word_set = BTreeSet::new();
        for t in &templates {
            word_set.insert(t.word.clone());
        }
        let words: Vec<String> = word_set.into_iter().collect();
        let mut word_to_instances: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, t) in templates.iter().enumerate() {
            word_to_instances.entry(t.word.clone()).or_default().push(i);
        }
        let sample_rate = templates.first().map(|t| t.sample_rate).unwrap_or(16_000);
        TemplateSet {
            templates,
            words,
            word_to_instances,
            sil_mean,
            sil_pool_frames,
            frame_shift_ms: 10.0,
            sample_rate,
        }
    }
}

/// Frozen-A1-Config per `hearsay-four-t5i--bounded-classical-baseline-topk`:
/// `base_cmn_d_dd_lifter22_pre097+cmvn_replaces_cmn+dtw_baseline+band0.2`.
/// Frontend part only (DTW variant is consumed by the decoder transitions).
pub fn frozen_frontend_config() -> FrontendConfig {
    FrontendConfig {
        pre_emphasis: 0.97,
        lifter: 22.0,
        cmn: true,
        cmvn: true,
        use_deltas: true,
        use_delta_deltas: true,
        mel_low_hz: 0.0,
        mel_high_hz: None,
        num_mfcc: 12,
    }
}

fn read_wav_mono(path: &Path) -> Result<(Vec<i16>, u32), String> {
    let mut r = hound::WavReader::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let spec = r.spec();
    let samples: Vec<i16> = r
        .samples::<i16>()
        .collect::<Result<_, _>>()
        .map_err(|e| format!("{e}"))?;
    let mono = if spec.channels > 1 {
        samples
            .chunks(spec.channels as usize)
            .map(|c| (c.iter().map(|&x| x as i32).sum::<i32>() / c.len() as i32) as i16)
            .collect()
    } else {
        samples
    };
    Ok((mono, spec.sample_rate))
}

/// Load templates from `dir/manifest.tsv` with `cfg`.
///
/// Skips `enroll.wav` rows and rows with empty labels (a1_gate convention).
/// Takes are per-label ordinals (first occurrence = take 0).
/// Returns Err (loud, never silent) on missing WAVs or empty feature output.
pub fn load_templates(dir: &Path, cfg: &FrontendConfig) -> Result<TemplateSet, String> {
    load_templates_with(dir, cfg, 8)
}

fn load_templates_with(
    dir: &Path,
    cfg: &FrontendConfig,
    sil_edge_frames: usize,
) -> Result<TemplateSet, String> {
    let manifest = dir.join("manifest.tsv");
    let text =
        std::fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut templates = Vec::new();
    let mut sil_acc: Vec<f64> = vec![0.0; cfg.dim()];
    let mut sil_n: usize = 0;
    for line in text.lines().skip(1) {
        let mut it = line.split('\t');
        let file = it.next().unwrap_or("");
        let label = it.next().unwrap_or("");
        if file == "enroll.wav" || label.is_empty() || file.is_empty() {
            continue;
        }
        let take = {
            let e = seen.entry(label.to_string()).or_insert(0);
            let t = *e;
            *e += 1;
            t
        };
        let wav = dir.join(file);
        if !wav.exists() {
            return Err(format!("MISSING {}", wav.display()));
        }
        let (samples, sr) = read_wav_mono(&wav)?;
        let frames = extract_with_config(&samples, sr, cfg).map_err(|e| format!("{file}: {e}"))?;
        if frames.is_empty() {
            return Err(format!("{file}: empty trimmed features (silent clip?)"));
        }
        // SIL pool from untrimmed lead/trail (recording silence, not speech).
        let untrimmed =
            extract_untrimmed_with_config(&samples, sr, cfg).map_err(|e| format!("{file}: {e}"))?;
        let k = sil_edge_frames.min(untrimmed.len() / 2);
        for f in untrimmed
            .iter()
            .take(k)
            .chain(untrimmed.iter().rev().take(k))
        {
            for (a, &x) in sil_acc.iter_mut().zip(f.iter()) {
                *a += x;
            }
            sil_n += 1;
        }
        templates.push(WordTemplate {
            word: label.to_string(),
            take,
            frames,
            samples,
            sample_rate: sr,
        });
    }
    if templates.is_empty() {
        return Err(format!("{}: no template rows", manifest.display()));
    }
    if sil_n == 0 {
        return Err("SIL pool empty (no lead/trail frames)".to_string());
    }
    let sil_mean: Vec<f64> = sil_acc.iter().map(|&a| a / sil_n as f64).collect();
    Ok(TemplateSet::index(templates, sil_mean, sil_n))
}

/// Load a vocab-agnostic bulk TTS dir (DEV-B machinery): every `*.wav`
/// is one take of the word before the first `_` (e.g. `abacus_0399.wav`).
/// Keeps the first `n_words` distinct labels sorted. No manifest needed.
pub fn load_bulk_templates(
    dir: &Path,
    cfg: &FrontendConfig,
    n_words: usize,
) -> Result<TemplateSet, String> {
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    let rd = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for e in rd {
        let e = e.map_err(|e| format!("{e}"))?;
        let p = e.path();
        if p.extension().map(|x| x == "wav").unwrap_or(false) {
            files.push(p);
        }
    }
    files.sort();
    let mut seen: HashMap<String, bool> = HashMap::new();
    let mut templates = Vec::new();
    let mut sil_acc: Vec<f64> = vec![0.0; cfg.dim()];
    let mut sil_n: usize = 0;
    for wav in files {
        if seen.len() >= n_words {
            break;
        }
        let stem = wav
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let label = stem.split('_').next().unwrap_or("").to_string();
        if label.is_empty() || seen.contains_key(&label) {
            continue;
        }
        seen.insert(label.clone(), true);
        let (samples, sr) = read_wav_mono(&wav)?;
        let frames = extract_with_config(&samples, sr, cfg)
            .map_err(|e| format!("{}: {e}", wav.display()))?;
        if frames.is_empty() {
            return Err(format!("{}: empty features", wav.display()));
        }
        let untrimmed = extract_untrimmed_with_config(&samples, sr, cfg)
            .map_err(|e| format!("{}: {e}", wav.display()))?;
        let k = 8.min(untrimmed.len() / 2);
        for f in untrimmed
            .iter()
            .take(k)
            .chain(untrimmed.iter().rev().take(k))
        {
            for (a, &x) in sil_acc.iter_mut().zip(f.iter()) {
                *a += x;
            }
            sil_n += 1;
        }
        templates.push(WordTemplate {
            word: label,
            take: 0,
            frames,
            samples,
            sample_rate: sr,
        });
    }
    if templates.is_empty() {
        return Err(format!("{}: no bulk wavs", dir.display()));
    }
    if sil_n == 0 {
        return Err("bulk SIL pool empty".to_string());
    }
    let sil_mean: Vec<f64> = sil_acc.iter().map(|&a| a / sil_n as f64).collect();
    Ok(TemplateSet::index(templates, sil_mean, sil_n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_config_dim_is_39() {
        assert_eq!(frozen_frontend_config().dim(), 39);
    }

    #[test]
    fn excluding_take_drops_only_heldout() {
        let mk = |word: &str, take: usize| WordTemplate {
            word: word.to_string(),
            take,
            frames: vec![vec![0.0; 3]],
            samples: vec![0],
            sample_rate: 16_000,
        };
        let set = TemplateSet::index(
            vec![mk("a", 0), mk("a", 1), mk("b", 0), mk("b", 1)],
            vec![0.0; 3],
            4,
        );
        let sub = set.excluding_take(0);
        assert_eq!(sub.templates.len(), 2);
        assert!(sub.templates.iter().all(|t| t.take == 1));
        assert_eq!(sub.words, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn load_templates_missing_dir_errors_loudly() {
        let cfg = frozen_frontend_config();
        let err = load_templates(Path::new("/nonexistent-a2-dir"), &cfg).unwrap_err();
        assert!(err.contains("manifest.tsv") || err.contains("nonexistent"));
    }
}
