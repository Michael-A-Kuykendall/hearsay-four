//! A2 DEV runner: connected-word decode + lattice + oracle/K sweep on DEV only.
//!
//! DEV-A (primary): `--template-dir` (run1-shaped manifest.tsv) + `--phrase-list`
//! (script-a2-phrases.txt). Templates exclude `--heldout-take`; phrases synth
//! from the held-out take only. Reads audio read-only; writes lattices + CSV
//! to `--out-dir` (use /tmp; only the MD summary lands in docs/results).
//!
//! DEV-B (TTS machinery): `--bulk-dir` + `--bulk-words` + `--bulk-phrases`;
//! vocab-agnostic smoke on bulk_tts_1k (same-clip concat, expect ~0 WER).
//!
//! Exit 0 with gate lines on stdout; loud Err (exit 1) on any failure.

use a2::concat::{PhraseSpec, parse_phrase_list, synth_phrase};
use a2::decoder::{DecoderConfig, decode, emit_lattice};
use a2::oracle::sweep_k;
use a2::templates::{TemplateSet, frozen_frontend_config, load_bulk_templates, load_templates};
use a2::wer::{WerSummary, edit_distance};
use fse::frontend::extract_untrimmed_with_config;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

struct Args {
    template_dir: Option<PathBuf>,
    phrase_list: Option<PathBuf>,
    out_dir: PathBuf,
    heldout_take: usize,
    phrase_take: Option<usize>,
    gap_ms: u32,
    lead_ms: u32,
    trail_ms: u32,
    beam: f64,
    wip: f64,
    max_live: usize,
    k_max: usize,
    limit: usize,
    bulk_dir: Option<PathBuf>,
    bulk_words: usize,
    bulk_phrases: usize,
    resweep: Option<PathBuf>,
    onebest_csv: Option<PathBuf>,
}

fn usage() -> ! {
    eprintln!(
        "usage: a2_dev --out-dir DIR [--template-dir D --phrase-list F] [--bulk-dir D] [opts]\n\
         opts: --heldout-take N (0) --gap-ms N (150) --beam F (400) --wip F (15)\n\
         --max-live N (2000) --k-max N (20) --limit N (0=all) --bulk-words N (10) --bulk-phrases N (20)"
    );
    std::process::exit(2);
}

fn parse_args() -> Args {
    let mut a = Args {
        template_dir: None,
        phrase_list: None,
        out_dir: PathBuf::from("/tmp/a2-dev"),
        heldout_take: 0,
        phrase_take: None,
        gap_ms: 150,
        lead_ms: 200,
        trail_ms: 200,
        beam: 400.0,
        wip: 15.0,
        max_live: 2000,
        k_max: 20,
        limit: 0,
        bulk_dir: None,
        bulk_words: 10,
        bulk_phrases: 20,
        resweep: None,
        onebest_csv: None,
    };
    let mut it = std::env::args().skip(1).peekable();
    while let Some(k) = it.next() {
        match k.as_str() {
            "--template-dir" => {
                a.template_dir = Some(PathBuf::from(it.next().unwrap_or_else(|| usage())))
            }
            "--phrase-list" => {
                a.phrase_list = Some(PathBuf::from(it.next().unwrap_or_else(|| usage())))
            }
            "--out-dir" => a.out_dir = PathBuf::from(it.next().unwrap_or_else(|| usage())),
            "--heldout-take" => {
                a.heldout_take = it
                    .next()
                    .unwrap_or_else(|| usage())
                    .parse()
                    .unwrap_or_else(|_| usage())
            }
            "--gap-ms" => {
                a.gap_ms = it
                    .next()
                    .unwrap_or_else(|| usage())
                    .parse()
                    .unwrap_or_else(|_| usage())
            }
            "--lead-ms" => {
                a.lead_ms = it
                    .next()
                    .unwrap_or_else(|| usage())
                    .parse()
                    .unwrap_or_else(|_| usage())
            }
            "--trail-ms" => {
                a.trail_ms = it
                    .next()
                    .unwrap_or_else(|| usage())
                    .parse()
                    .unwrap_or_else(|_| usage())
            }
            "--phrase-take" => {
                a.phrase_take = Some(
                    it.next()
                        .unwrap_or_else(|| usage())
                        .parse()
                        .unwrap_or_else(|_| usage()),
                )
            }
            "--beam" => {
                a.beam = it
                    .next()
                    .unwrap_or_else(|| usage())
                    .parse()
                    .unwrap_or_else(|_| usage())
            }
            "--wip" => {
                a.wip = it
                    .next()
                    .unwrap_or_else(|| usage())
                    .parse()
                    .unwrap_or_else(|_| usage())
            }
            "--max-live" => {
                a.max_live = it
                    .next()
                    .unwrap_or_else(|| usage())
                    .parse()
                    .unwrap_or_else(|_| usage())
            }
            "--k-max" => {
                a.k_max = it
                    .next()
                    .unwrap_or_else(|| usage())
                    .parse()
                    .unwrap_or_else(|_| usage())
            }
            "--limit" => {
                a.limit = it
                    .next()
                    .unwrap_or_else(|| usage())
                    .parse()
                    .unwrap_or_else(|_| usage())
            }
            "--bulk-dir" => a.bulk_dir = Some(PathBuf::from(it.next().unwrap_or_else(|| usage()))),
            "--bulk-words" => {
                a.bulk_words = it
                    .next()
                    .unwrap_or_else(|| usage())
                    .parse()
                    .unwrap_or_else(|_| usage())
            }
            "--bulk-phrases" => {
                a.bulk_phrases = it
                    .next()
                    .unwrap_or_else(|| usage())
                    .parse()
                    .unwrap_or_else(|_| usage())
            }
            "--resweep" => a.resweep = Some(PathBuf::from(it.next().unwrap_or_else(|| usage()))),
            "--onebest-csv" => {
                a.onebest_csv = Some(PathBuf::from(it.next().unwrap_or_else(|| usage())))
            }
            _ => usage(),
        }
    }
    if a.template_dir.is_none() && a.bulk_dir.is_none() && a.resweep.is_none() {
        usage();
    }
    if a.template_dir.is_some() && a.phrase_list.is_none() {
        usage();
    }
    if a.resweep.is_some() && a.phrase_list.is_none() {
        usage();
    }
    a
}

fn decoder_config(a: &Args) -> DecoderConfig {
    DecoderConfig {
        beam: a.beam,
        max_live: a.max_live,
        wip: a.wip,
        ..DecoderConfig::default()
    }
}

fn median(mut xs: Vec<f64>) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.sort_by(|x, y| x.partial_cmp(y).unwrap());
    let n = xs.len();
    if n % 2 == 1 {
        xs[n / 2]
    } else {
        (xs[n / 2 - 1] + xs[n / 2]) / 2.0
    }
}

fn run_dev_a(a: &Args, cfg: &fse::frontend::FrontendConfig) -> Result<(), String> {
    let tdir = a.template_dir.as_ref().unwrap();
    let full = load_templates(tdir, cfg)?;
    let dec_set = full.excluding_take(a.heldout_take);
    println!(
        "DEV-A templates: {} clips ({} words), heldout take {} excluded; SIL pool {} frames",
        dec_set.templates.len(),
        dec_set.words.len(),
        a.heldout_take,
        dec_set.sil_pool_frames
    );
    // Phrase clip index: phrase take (default = held-out take; override with
    // --phrase-take for the same-take control: --heldout-take 99 --phrase-take 0).
    let ptake = a.phrase_take.unwrap_or(a.heldout_take);
    let mut clips: HashMap<(String, usize), (Vec<i16>, u32)> = HashMap::new();
    for t in full.templates.iter().filter(|t| t.take == ptake) {
        clips.insert((t.word.clone(), ptake), (t.samples.clone(), t.sample_rate));
    }
    println!("DEV-A phrase take: {ptake} ({} clips)", clips.len());
    let plist = std::fs::read_to_string(a.phrase_list.as_ref().unwrap())
        .map_err(|e| format!("phrase-list: {e}"))?;
    let mut specs = parse_phrase_list(&plist);
    if a.limit > 0 {
        specs.truncate(a.limit);
    }
    println!("DEV-A phrases: {} (limit {})", specs.len(), a.limit);
    run_phrases(a, cfg, &dec_set, &clips, &specs, "deva")
}

fn run_phrases(
    a: &Args,
    cfg: &fse::frontend::FrontendConfig,
    dec_set: &TemplateSet,
    clips: &HashMap<(String, usize), (Vec<i16>, u32)>,
    specs: &[PhraseSpec],
    tag: &str,
) -> Result<(), String> {
    let dc = decoder_config(a);
    let lat_dir = a.out_dir.join("lattices").join(tag);
    std::fs::create_dir_all(&lat_dir).map_err(|e| format!("mkdir: {e}"))?;
    let mut rows: Vec<String> = vec!["id,nref,nhyp,errors,boundary_med_ms,frames,peak_live,decode_ms,audio_ms,rtf,cost,norm,n_events".to_string()];
    let mut pairs: Vec<(u32, u32)> = Vec::new();
    let mut oracle_pairs: HashMap<usize, Vec<(u32, u32)>> = HashMap::new();
    for k in [3usize, 5, 10, 20] {
        if k <= a.k_max {
            oracle_pairs.insert(k, Vec::new());
        }
    }
    let mut all_bound: Vec<f64> = Vec::new();
    let mut peak = 0usize;
    let mut fails = 0usize;
    for spec in specs {
        let take = clips
            .keys()
            .next()
            .map(|(_, t)| *t)
            .unwrap_or(a.heldout_take);
        let synth = synth_phrase(spec, clips, take, a.gap_ms, a.lead_ms, a.trail_ms)?;
        let feats = extract_untrimmed_with_config(&synth.samples, synth.sample_rate, cfg)
            .map_err(|e| format!("{}: {e}", spec.id))?;
        if feats.is_empty() {
            return Err(format!("{}: empty phrase features", spec.id));
        }
        let audio_ms = synth.samples.len() as f64 / synth.sample_rate as f64 * 1000.0;
        let t0 = Instant::now();
        let res = match decode(dec_set, &feats, &dc) {
            Ok(r) => r,
            Err(e) => {
                fails += 1;
                eprintln!("{}: DECODE-FAIL {e}", spec.id);
                continue;
            }
        };
        let dt_ms = t0.elapsed().as_secs_f64() * 1000.0;
        peak = peak.max(res.peak_live);
        let hyp: Vec<String> = res.words.iter().map(|w| w.word.clone()).collect();
        let err = edit_distance(&spec.words, &hyp);
        pairs.push((err, spec.words.len() as u32));
        // Boundary error vs exact synth bounds (matched words by position).
        let mut berr: Vec<f64> = Vec::new();
        for (i, w) in res.words.iter().enumerate() {
            if i < synth.bounds.len() && synth.bounds[i].0 == w.word {
                let (rs, re) = (synth.bounds[i].1, synth.bounds[i].2);
                berr.push(((w.start as f64 - rs as f64) * 10.0).abs());
                berr.push(((w.end as f64 - re as f64) * 10.0).abs());
            }
        }
        let bmed = median(berr.clone());
        all_bound.extend(berr);
        // Lattice (K=a.k_max) + sweep.
        let lat = emit_lattice(
            &spec.id,
            &res.events,
            &res.words,
            a.k_max,
            dc.max_sp_frames,
            10.0,
        )?;
        let text = lattice::write_lattice(&lat)?;
        std::fs::write(lat_dir.join(format!("{}.lat.jsonl", spec.id)), &text)
            .map_err(|e| format!("write lattice: {e}"))?;
        let ks: Vec<usize> = oracle_pairs.keys().cloned().collect();
        let sw = sweep_k(&text, &spec.words, &ks)?;
        for (k, o, _, _) in sw {
            oracle_pairs
                .get_mut(&k)
                .unwrap()
                .push((o.errors, o.ref_len));
        }
        rows.push(format!(
            "{},{},{},{},{:.1},{},{},{:.0},{:.0},{:.3},{:.1},{:.3},{}",
            spec.id,
            spec.words.len(),
            hyp.len(),
            err,
            bmed,
            res.frames,
            res.peak_live,
            dt_ms,
            audio_ms,
            dt_ms / audio_ms.max(1.0),
            res.cost,
            res.norm_cost,
            res.events.len()
        ));
        println!(
            "{} hyp=[{}] err={} bmed={:.0}ms live={} ev={}",
            spec.id,
            hyp.join(" "),
            err,
            bmed,
            res.peak_live,
            res.events.len()
        );
    }
    let csv = a.out_dir.join(format!("per_phrase_{tag}.csv"));
    std::fs::write(&csv, rows.join("\n")).map_err(|e| format!("write csv: {e}"))?;
    let s = WerSummary::from_pairs(&pairs);
    println!(
        "GATE {tag} onebest: errs={} refs={} WER={:.4} n={} fails={}",
        s.errors, s.ref_words, s.wer, s.utterances, fails
    );
    println!(
        "GATE {tag} boundary_med_ms={:.1} peak_live={} (bound {})",
        median(all_bound),
        peak,
        a.max_live
    );
    let mut ks: Vec<usize> = oracle_pairs.keys().cloned().collect();
    ks.sort();
    for k in ks {
        let v = &oracle_pairs[&k];
        let so = WerSummary::from_pairs(v);
        println!(
            "GATE {tag} oracleK{k}: errs={} refs={} WER={:.4} n={}",
            so.errors, so.ref_words, so.wer, so.utterances
        );
    }
    // CI: 1-best vs oracle@kmax delta (corpus_delta_ci, adapt=oracle).
    if let Some(v) = oracle_pairs.get(&a.k_max)
        && v.len() == pairs.len()
        && !pairs.is_empty()
    {
        let base: Vec<u32> = pairs.iter().map(|&(e, _)| e).collect();
        let adapt: Vec<u32> = v.iter().map(|&(e, _)| e).collect();
        let den: Vec<u32> = pairs.iter().map(|&(_, d)| d).collect();
        let ci = eval::corpus_delta_ci(
            &base,
            &adapt,
            &den,
            eval::BootstrapConfig {
                resamples: 2000,
                seed: 20261031,
                confidence: 0.95,
            },
        );
        println!(
            "GATE {tag} delta_oracle_minus_1best={:.4} CI95=[{:.4},{:.4}]",
            ci.estimate, ci.lo, ci.hi
        );
    }
    if fails > 0 {
        return Err(format!("{tag}: {fails} decode failures"));
    }
    Ok(())
}

/// Re-sweep existing lattices (no decode): reads `<id>.lat.jsonl` from
/// `--resweep` DIR + refs from `--phrase-list`, prints oracle GATE lines.
/// 1-best pairs for the delta CI come from `--onebest-csv` (per_phrase CSV).
fn run_resweep(a: &Args) -> Result<(), String> {
    let dir = a.resweep.as_ref().unwrap();
    let plist = std::fs::read_to_string(a.phrase_list.as_ref().unwrap())
        .map_err(|e| format!("phrase-list: {e}"))?;
    let specs = parse_phrase_list(&plist);
    // 1-best pairs from CSV (optional).
    let mut onebest: HashMap<String, (u32, u32)> = HashMap::new();
    if let Some(csv) = a.onebest_csv.as_ref() {
        let text = std::fs::read_to_string(csv).map_err(|e| format!("onebest-csv: {e}"))?;
        for line in text.lines().skip(1) {
            let f: Vec<&str> = line.split(',').collect();
            if f.len() >= 4
                && let (Ok(e), Ok(d)) = (f[3].parse::<u32>(), f[1].parse::<u32>())
            {
                onebest.insert(f[0].to_string(), (e, d));
            }
        }
    }
    let ks: Vec<usize> = [3usize, 5, 10, 20]
        .into_iter()
        .filter(|k| *k <= a.k_max)
        .collect();
    let mut agg: HashMap<usize, Vec<(u32, u32)>> = HashMap::new();
    for k in &ks {
        agg.insert(*k, Vec::new());
    }
    let mut n = 0usize;
    let mut sizes: HashMap<usize, (u64, u64)> = HashMap::new();
    for spec in &specs {
        let p = dir.join(format!("{}.lat.jsonl", spec.id));
        let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        // CONF line for 1-best error phrases (backbone hyp from lattice).
        if onebest.get(&spec.id).map(|&(e, _)| e).unwrap_or(0) > 0 {
            let lat = lattice::read_lattice(&text)?;
            let mut bb: Vec<&lattice::Token> = lat
                .tokens
                .iter()
                .filter(|t| t.source == "a2-1best")
                .collect();
            bb.sort_by_key(|t| (t.start, t.end));
            let hyp: Vec<&str> = bb.iter().map(|t| t.word.as_str()).collect();
            println!(
                "CONF {} ref=[{}] hyp=[{}]",
                spec.id,
                spec.words.join(" "),
                hyp.join(" ")
            );
        }
        for (k, o, nt, na) in sweep_k(&text, &spec.words, &ks)? {
            agg.get_mut(&k).unwrap().push((o.errors, o.ref_len));
            let e = sizes.entry(k).or_insert((0, 0));
            e.0 += nt as u64;
            e.1 += na as u64;
            if k == a.k_max {
                println!(
                    "{} oracle errs={} refs={} tok={} arcs={}",
                    spec.id, o.errors, o.ref_len, nt, na
                );
            }
        }
        n += 1;
    }
    let mut sks: Vec<usize> = agg.keys().cloned().collect();
    sks.sort();
    for k in sks {
        let s = WerSummary::from_pairs(&agg[&k]);
        let (tt, ta) = sizes.get(&k).cloned().unwrap_or((0, 0));
        println!(
            "GATE resweep oracleK{k}: errs={} refs={} WER={:.4} n={} meantok={:.0} meanarcs={:.0}",
            s.errors,
            s.ref_words,
            s.wer,
            n,
            tt as f64 / n.max(1) as f64,
            ta as f64 / n.max(1) as f64
        );
    }
    if let Some(v) = agg.get(&a.k_max) {
        let mut base = Vec::new();
        let mut adapt = Vec::new();
        let mut den = Vec::new();
        for (i, spec) in specs.iter().enumerate() {
            if let Some(&(e, d)) = onebest.get(&spec.id) {
                base.push(e);
                adapt.push(v[i].0);
                den.push(d);
            }
        }
        if !base.is_empty() {
            let ci = eval::corpus_delta_ci(
                &base,
                &adapt,
                &den,
                eval::BootstrapConfig {
                    resamples: 2000,
                    seed: 20261031,
                    confidence: 0.95,
                },
            );
            println!(
                "GATE resweep delta_oracle_minus_1best={:.4} CI95=[{:.4},{:.4}]",
                ci.estimate, ci.lo, ci.hi
            );
        }
    }
    Ok(())
}

fn run_dev_b(a: &Args, cfg: &fse::frontend::FrontendConfig) -> Result<(), String> {
    let bdir = a.bulk_dir.as_ref().unwrap();
    let set = load_bulk_templates(bdir, cfg, a.bulk_words)?;
    println!(
        "DEV-B bulk templates: {} words from {}",
        set.templates.len(),
        bdir.display()
    );
    // Deterministic round-robin phrases over bulk vocab (same-clip concat).
    let mut clips: HashMap<(String, usize), (Vec<i16>, u32)> = HashMap::new();
    for t in &set.templates {
        clips.insert((t.word.clone(), 0), (t.samples.clone(), t.sample_rate));
    }
    let mut specs = Vec::new();
    for i in 0..a.bulk_phrases {
        let n = 3 + (i % 3); // 3-5 words
        let mut words = Vec::new();
        for j in 0..n {
            words.push(set.words[(i + j) % set.words.len()].clone());
        }
        specs.push(PhraseSpec {
            id: format!("bulk{:03}", i),
            words,
        });
    }
    run_phrases(a, cfg, &set, &clips, &specs, "devb")
}

fn main() {
    let a = parse_args();
    let cfg = frozen_frontend_config();
    let mut failed = false;
    if a.template_dir.is_some()
        && let Err(e) = run_dev_a(&a, &cfg)
    {
        eprintln!("DEV-A FAILED: {e}");
        failed = true;
    }
    if a.bulk_dir.is_some()
        && let Err(e) = run_dev_b(&a, &cfg)
    {
        eprintln!("DEV-B FAILED: {e}");
        failed = true;
    }
    if a.resweep.is_some()
        && let Err(e) = run_resweep(&a)
    {
        eprintln!("RESWEEP FAILED: {e}");
        failed = true;
    }
    if failed {
        std::process::exit(1);
    }
}
