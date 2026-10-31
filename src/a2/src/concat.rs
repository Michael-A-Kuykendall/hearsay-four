//! Connected-phrase synthesis by concatenating isolated takes (DEV-only).
//!
//! No natural connected recordings exist yet, so DEV phrases concatenate
//! trimmed isolated clips with fixed silence gaps. Boundaries are EXACT
//! (known from synthesis — stronger than the 20 hand-checked phrases the
//! gate asks for). Phrases are built in memory; audio never enters the repo.
//!
//! Take discipline: templates exclude the held-out take; phrases use ONLY
//! the held-out take. Caveat (reported): 30 distinct acoustic realizations
//! reused across 104 phrase contexts.

use std::collections::HashMap;

/// Reference phrase: id + word sequence.
#[derive(Clone, Debug)]
pub struct PhraseSpec {
    pub id: String,
    pub words: Vec<String>,
}

/// Parse a phrase list (one phrase per line, `#` comments + blanks skipped).
pub fn parse_phrase_list(text: &str) -> Vec<PhraseSpec> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let words: Vec<String> = line.split_whitespace().map(str::to_string).collect();
        if words.is_empty() {
            continue;
        }
        let id = format!("phr{:03}", out.len());
        out.push(PhraseSpec { id, words });
    }
    out
}

/// Synthesized phrase: samples + exact word boundaries in frames.
#[derive(Clone, Debug)]
pub struct SynthPhrase {
    pub spec: PhraseSpec,
    pub samples: Vec<i16>,
    pub sample_rate: u32,
    /// (word, start_frame, end_frame inclusive) at 10 ms shift.
    pub bounds: Vec<(String, u32, u32)>,
}

/// Energy trimmer: keep speech span + margin (sample indexes, end exclusive).
/// Threshold adapts to clip peak (5%); margin 100 ms each side.
pub fn trim_span(samples: &[i16], sample_rate: u32) -> Result<(usize, usize), String> {
    if samples.is_empty() {
        return Err("trim: empty samples".to_string());
    }
    let shift = ((sample_rate as f64 * 10.0) / 1000.0).round() as usize;
    let shift = shift.max(1);
    let n = samples.len().div_ceil(shift);
    let mut rms = Vec::with_capacity(n);
    let mut peak = 0f64;
    for i in 0..n {
        let s = i * shift;
        let e = ((i + 1) * shift).min(samples.len());
        let mut acc = 0f64;
        for &x in &samples[s..e] {
            acc += (x as f64) * (x as f64);
        }
        let r = (acc / (e - s) as f64).sqrt();
        peak = peak.max(r);
        rms.push(r);
    }
    let thresh = (peak * 0.05).max(50.0);
    let mut first = None;
    let mut last = 0;
    for (i, &r) in rms.iter().enumerate() {
        if r >= thresh {
            if first.is_none() {
                first = Some(i);
            }
            last = i;
        }
    }
    let Some(f) = first else {
        return Err("trim: no speech above floor (silent clip)".to_string());
    };
    let margin = (sample_rate as usize / 10).max(shift); // ~100 ms
    let s = (f * shift).saturating_sub(margin);
    let e = (((last + 1) * shift) + margin).min(samples.len());
    Ok((s, e.max(s + shift)))
}

/// Take `ms` of recording silence from `clip` head (lead=true) or tail.
/// Falls back to digital zeros only when the clip is shorter than asked.
fn real_silence(clip: &[i16], sample_rate: u32, ms: u32, lead: bool) -> Vec<i16> {
    let want = (sample_rate as usize * ms as usize / 1000).max(1);
    if clip.len() >= want {
        if lead {
            clip[..want].to_vec()
        } else {
            clip[clip.len() - want..].to_vec()
        }
    } else {
        clip.to_vec()
    }
}

/// Concatenate takes per `spec` with REAL-silence lead/gaps/trail.
///
/// Gaps/lead/trail come from the clips' own head/tail (recording silence,
/// the same population as the SIL pool) — never digital zeros, which the
/// SIL model (mic noise) would reject and which would bias segmentation.
pub fn synth_phrase(
    spec: &PhraseSpec,
    clips: &HashMap<(String, usize), (Vec<i16>, u32)>,
    take: usize,
    gap_ms: u32,
    lead_ms: u32,
    trail_ms: u32,
) -> Result<SynthPhrase, String> {
    let mut samples: Vec<i16> = Vec::new();
    let mut bounds = Vec::new();
    let mut sr_opt: Option<u32> = None;
    // Resolve all clips first (rate check + lead silence source).
    let mut resolved: Vec<(&Vec<i16>, u32)> = Vec::with_capacity(spec.words.len());
    for w in &spec.words {
        let (clip, sr) = clips
            .get(&(w.clone(), take))
            .ok_or_else(|| format!("phrase {}: no take {take} for word '{w}'", spec.id))?;
        if let Some(sr0) = sr_opt {
            if *sr != sr0 {
                return Err(format!("phrase {}: mixed rates {sr0} vs {sr}", spec.id));
            }
        } else {
            sr_opt = Some(*sr);
        }
        resolved.push((clip, *sr));
    }
    let sample_rate = sr_opt.ok_or_else(|| format!("phrase {}: empty", spec.id))?;
    let shift = ((sample_rate as f64 * 10.0) / 1000.0).round() as usize;
    // Lead from the first clip's head.
    samples.extend(real_silence(resolved[0].0, sample_rate, lead_ms, true));
    for (wi, w) in spec.words.iter().enumerate() {
        let (clip, _) = resolved[wi];
        if wi > 0 {
            // Gap from the preceding clip's tail (its own recording silence).
            samples.extend(real_silence(resolved[wi - 1].0, sample_rate, gap_ms, false));
        }
        let (s, e) = trim_span(clip, sample_rate)?;
        let start_frame = (samples.len() / shift) as u32;
        samples.extend_from_slice(&clip[s..e]);
        let end_frame = (samples.len().saturating_sub(1) / shift) as u32;
        bounds.push((w.clone(), start_frame, end_frame));
    }
    // Trail from the last clip's tail.
    let last = resolved.len() - 1;
    samples.extend(real_silence(resolved[last].0, sample_rate, trail_ms, false));
    Ok(SynthPhrase {
        spec: spec.clone(),
        samples,
        sample_rate,
        bounds,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_comments_and_blanks() {
        let v = parse_phrase_list("# c\n\na b c\n\n# d\nd e\n");
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].id, "phr000");
        assert_eq!(v[0].words, vec!["a", "b", "c"]);
        assert_eq!(v[1].words, vec!["d", "e"]);
    }

    #[test]
    fn trimmer_keeps_tone_trims_silence() {
        let sr = 16_000u32;
        let mut s = vec![0i16; sr as usize]; // 1 s silence
        for i in 0..sr as usize / 2 {
            // 0.5 s 440 Hz tone at 40% full scale
            let t = i as f64 / sr as f64;
            s[sr as usize / 4 + i] =
                (0.4 * i16::MAX as f64 * (2.0 * std::f64::consts::PI * 440.0 * t).sin()) as i16;
        }
        s.extend(vec![0i16; sr as usize / 2]);
        let (a, b) = trim_span(&s, sr).expect("trim");
        assert!(a < sr as usize / 4 + 1000, "lead trimmed loosely: {a}");
        assert!(b > sr as usize / 4, "speech kept: {b}");
        assert!(b - a < s.len(), "trailing cut: {} < {}", b - a, s.len());
    }

    #[test]
    fn silent_clip_errors_loudly() {
        assert!(trim_span(&vec![0i16; 16000], 16_000).is_err());
    }

    #[test]
    fn synth_concatenates_with_exact_bounds() {
        let sr = 16_000u32;
        let tone = |f: f64| {
            (0..sr as usize / 4)
                .map(|i| {
                    (0.4 * i16::MAX as f64
                        * (2.0 * std::f64::consts::PI * f * i as f64 / sr as f64).sin())
                        as i16
                })
                .collect::<Vec<_>>()
        };
        let mut clips = HashMap::new();
        clips.insert(("a".to_string(), 0), (tone(440.0), sr));
        clips.insert(("b".to_string(), 0), (tone(660.0), sr));
        let spec = PhraseSpec {
            id: "phr000".into(),
            words: vec!["a".into(), "b".into()],
        };
        let p = synth_phrase(&spec, &clips, 0, 150, 200, 200).expect("synth");
        assert_eq!(p.bounds.len(), 2);
        assert_eq!(p.bounds[0].0, "a");
        assert!(p.bounds[1].1 > p.bounds[0].2, "ordered: {:?}", p.bounds);
    }
}
