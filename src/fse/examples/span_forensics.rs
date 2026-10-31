//! Span forensics: energy plot + endpointer span for one raw take, plus a
//! canonical duration audit (each phase-test take vs its run1 word median).
//! Usage: span_forensics <raw-wav> <canon-run1-dir> <canon-ptest-dir>
use std::collections::HashMap;
use std::path::PathBuf;

fn frame_db(pcm: &[i16], rate: u32) -> Vec<f64> {
    let win = (rate as usize * 25) / 1000;
    let shift = (rate as usize * 10) / 1000;
    let mut out = Vec::new();
    let mut i = 0;
    while i + win <= pcm.len() {
        let mut s = 0.0;
        for &x in &pcm[i..i + win] {
            let v = x as f64 / 32768.0;
            s += v * v;
        }
        let rms = (s / win as f64).sqrt().max(1e-9);
        out.push(20.0 * rms.log10());
        i += shift;
    }
    out
}

fn main() {
    let wav = std::env::args()
        .nth(1)
        .expect("usage: span_forensics <raw-wav> <canon-run1> <canon-ptest>");
    let run1dir = std::env::args().nth(2).expect("canon-run1 dir");
    let ptestdir = std::env::args().nth(3).expect("canon-ptest dir");
    let pcm = corpus::audio::decode_to_pcm_16k_mono(&PathBuf::from(&wav)).expect("decode");
    let db = frame_db(&pcm, 16000);
    let peak = pcm
        .iter()
        .map(|x| x.abs() as f64 / 32768.0)
        .fold(0.0f64, f64::max);
    println!(
        "FILE {} samples={} secs={:.2} peak_dbfs={:.1}",
        wav,
        pcm.len(),
        pcm.len() as f64 / 16000.0,
        20.0 * peak.max(1e-9).log10()
    );
    let peak_db = db.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    println!("ENERGY rows = 100ms buckets, bar 60dB range, peak {peak_db:.1}dB");
    for (r, chunk) in db.chunks(10).enumerate() {
        let m = chunk.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let n = ((m - (peak_db - 60.0)).clamp(0.0, 60.0) / 60.0 * 60.0) as usize;
        println!("{:4}ms {:6.1}dB {}", r * 100, m, "#".repeat(n));
    }
    match corpus::canonical::endpoint_span(&pcm) {
        Ok(s) => println!(
            "SPAN start_sample={} end_sample={} speech_ms={}",
            s.start_sample,
            s.end_sample,
            (s.end_sample - s.start_sample) * 1000 / 16000
        ),
        Err(e) => println!("SPAN REJECTED: {e}"),
    }

    // Duration audit: run1 word medians vs phase-test takes.
    let run1 = corpus::canonical::load_session(&PathBuf::from(&run1dir)).expect("run1");
    let mut by_word: HashMap<String, Vec<usize>> = HashMap::new();
    for t in &run1.takes {
        by_word.entry(t.prompt.clone()).or_default().push(t.pcm.len());
    }
    let mut med: HashMap<String, usize> = HashMap::new();
    for (w, mut v) in by_word {
        v.sort();
        med.insert(w, v[v.len() / 2]);
    }
    let pt = corpus::canonical::load_session(&PathBuf::from(&ptestdir)).expect("ptest");
    println!("DUR-AUDIT ptest_takes={} (flag if < 60% of run1 word median)", pt.takes.len());
    let mut flags = 0;
    for t in &pt.takes {
        let m = med.get(&t.prompt).copied().unwrap_or(0);
        let ratio = if m > 0 { t.pcm.len() as f64 / m as f64 } else { -1.0 };
        let flag = if m > 0 && ratio < 0.6 { " FLAG" } else { "" };
        if !flag.is_empty() {
            flags += 1;
        }
        println!(
            "  {} prompt={} ms={} run1med_ms={} ratio={:.2}{}",
            t.id,
            t.prompt,
            t.pcm.len() * 1000 / 16000,
            m * 1000 / 16000,
            ratio,
            flag
        );
    }
    println!("DUR-AUDIT flags={flags}");
}
