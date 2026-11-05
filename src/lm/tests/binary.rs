//! Binary LM format gate tests (bead hearsay-four-6qn): JSON -> binary ->
//! load round trips with identical counts, encoding determinism, and the
//! 72MB r1v2-full binary load under the 2s wall bound.

use lm::persist;
use lm::{InterpolatedLm, KnTrigram, normalize_sentence};

fn sents(texts: &[&str]) -> Vec<Vec<String>> {
    texts.iter().map(|s| normalize_sentence(s)).collect()
}

fn general() -> KnTrigram {
    KnTrigram::train(&sents(&[
        "the cat sat on the mat",
        "the dog sat on the rug",
        "a cat ran to the mat",
        "the dog ran to the rug",
        "cats and dogs are animals",
        "the bird flew over the mat",
    ]))
}

fn personal() -> KnTrigram {
    KnTrigram::train(&sents(&[
        "the court held that the statute applies",
        "the court affirmed the judgment below",
        "counsel argued that the statute applies",
        "the judgment was affirmed by the court",
    ]))
}

fn assert_same_counts(a: &KnTrigram, b: &KnTrigram) {
    let (x, y) = (a.inspectable(), b.inspectable());
    assert_eq!(x.vocabulary, y.vocabulary);
    assert_eq!(x.unigram_counts, y.unigram_counts);
    assert_eq!(x.bigram_counts, y.bigram_counts);
    assert_eq!(x.trigram_counts, y.trigram_counts);
    assert_eq!(x.total_tokens, y.total_tokens);
    assert_eq!(x.trigram_discounts, y.trigram_discounts);
    assert_eq!(x.bigram_discounts, y.bigram_discounts);
}

const PROBES: [(&str, &str, &str); 4] = [
    ("the", "cat", "sat"),
    ("the", "court", "held"),
    ("zebra", "cat", "sat"),
    ("<s>", "<s>", "the"),
];

#[test]
fn binary_kn_json_roundtrip_counts_identical() {
    let model = general();
    // JSON -> load -> binary -> load: counts identical end to end.
    let json = persist::kn_to_bytes(&model);
    let from_json = persist::kn_from_bytes(&json).expect("json reload");
    let bin = persist::kn_to_binary_bytes(&from_json);
    assert!(lm::binary::is_binary_archive(&bin));
    assert!(!lm::binary::is_binary_archive(&json));
    let from_bin = persist::kn_from_binary_bytes(&bin).expect("binary reload");
    assert_same_counts(&model, &from_bin);
    for (w1, w2, w) in PROBES {
        assert_eq!(
            model.prob(w1, w2, w),
            from_bin.prob(w1, w2, w),
            "prob for ({w1},{w2},{w})"
        );
    }
    // File-level save/load agrees too.
    let tmp =
        std::env::temp_dir().join(format!("hearsay-6qn-binary-kn-{}.hslm", std::process::id()));
    persist::save_kn_binary(&model, &tmp).expect("save binary");
    let from_file = persist::load_kn_binary(&tmp).expect("load binary file");
    let _ = std::fs::remove_file(&tmp);
    assert_same_counts(&model, &from_file);
}

#[test]
fn binary_interpolated_json_roundtrip_counts_identical() {
    let lm = InterpolatedLm::new(general(), personal(), 0.3).expect("interpolated weight 0.3");
    let json = persist::interpolated_to_bytes(&lm);
    let from_json = persist::interpolated_from_bytes(&json).expect("json reload");
    let bin = persist::interpolated_to_binary_bytes(&from_json);
    let from_bin = persist::interpolated_from_binary_bytes(&bin).expect("binary reload");
    assert_eq!(lm.personal_weight(), from_bin.personal_weight());
    assert_same_counts(lm.general(), from_bin.general());
    assert_same_counts(lm.personal(), from_bin.personal());
    for (w1, w2, w) in PROBES {
        assert_eq!(
            lm.prob(w1, w2, w),
            from_bin.prob(w1, w2, w),
            "prob for ({w1},{w2},{w})"
        );
    }
}

#[test]
fn binary_encoding_is_deterministic() {
    let lm = InterpolatedLm::new(general(), personal(), 0.3).expect("weight 0.3");
    assert_eq!(
        persist::kn_to_binary_bytes(lm.general()),
        persist::kn_to_binary_bytes(lm.general())
    );
    assert_eq!(
        persist::interpolated_to_binary_bytes(&lm),
        persist::interpolated_to_binary_bytes(&lm)
    );
}

#[test]
fn binary_rejects_wrong_magic_and_truncation() {
    let bin = persist::kn_to_binary_bytes(&general());
    assert!(persist::kn_from_binary_bytes(b"nope").is_err());
    assert!(persist::kn_from_binary_bytes(&bin[..bin.len() / 2]).is_err());
    // JSON bytes are not a binary archive; kind mismatch is rejected.
    let json = persist::kn_to_bytes(&general());
    assert!(persist::kn_from_binary_bytes(&json).is_err());
    assert!(persist::interpolated_from_binary_bytes(&bin).is_err());
}

#[test]
fn binary_load_r1v2_full_under_2s() {
    let json_path =
        "/home/michael/repos/hearsay-four/data/lm-text/r1v2-full/lm-r1v2-interpolated.json";
    let json_bytes = std::fs::read(json_path).expect("read 72MB r1v2-full json");
    // Untimed setup: the JSON load is the slow path this format replaces.
    let t0 = std::time::Instant::now();
    let lm = persist::interpolated_from_bytes(&json_bytes).expect("json load");
    let json_secs = t0.elapsed().as_secs_f64();
    drop(json_bytes);
    let t0 = std::time::Instant::now();
    let bin = persist::interpolated_to_binary_bytes(&lm);
    let encode_secs = t0.elapsed().as_secs_f64();
    let tmp = std::env::temp_dir().join(format!(
        "hearsay-6qn-binary-r1v2-{}.hslm",
        std::process::id()
    ));
    persist::save_interpolated_binary(&lm, &tmp).expect("save binary");
    let bin_disk_len = std::fs::metadata(&tmp).expect("stat binary").len();
    // Timed gate: binary file load must clear the 2s wall bound.
    let t0 = std::time::Instant::now();
    let from_file = persist::load_interpolated_binary(&tmp).expect("binary file load");
    let file_secs = t0.elapsed().as_secs_f64();
    let t0 = std::time::Instant::now();
    let from_bytes = persist::interpolated_from_binary_bytes(&bin).expect("binary bytes load");
    let bytes_secs = t0.elapsed().as_secs_f64();
    let _ = std::fs::remove_file(&tmp);
    assert_same_counts(lm.general(), from_file.general());
    assert_same_counts(lm.personal(), from_file.personal());
    assert_same_counts(lm.general(), from_bytes.general());
    assert_same_counts(lm.personal(), from_bytes.personal());
    assert_eq!(lm.personal_weight(), from_file.personal_weight());
    println!(
        "r1v2-full binary gate: json_secs={json_secs:.2} encode_secs={encode_secs:.2} \
         file_secs={file_secs:.3} bytes_secs={bytes_secs:.3} bin_bytes={} disk_bytes={bin_disk_len} \
         general_bi={} general_tri={} personal_bi={} personal_tri={}",
        bin.len(),
        from_file.general().bigram_entry_count(),
        from_file.general().trigram_entry_count(),
        from_file.personal().bigram_entry_count(),
        from_file.personal().trigram_entry_count(),
    );
    assert!(
        file_secs < 2.0,
        "binary file load {file_secs:.3}s exceeds 2s bound"
    );
    assert!(
        bytes_secs < 2.0,
        "binary bytes load {bytes_secs:.3}s exceeds 2s bound"
    );
}
