use crate::manifest::{parse_librispeech_tsv, sample_stratified};

const FIXTURE: &str = "\
a-1-0000\tr/a-1-0000.wav\t1.0\tHELLO WORLD\n\
a-1-0001\tr/a-1-0001.wav\t2.0\tFOO BAR\n\
a-2-0000\tr/a-2-0000.wav\t3.0\tBAZ\n\
b-1-0000\tr/b-1-0000.wav\t4.0\tQUX QUUX\n\
b-1-0001\tr/b-1-0001.wav\t5.0\tNOPE\n\
\n\
bad-row-no-tabs\n";

#[test]
fn parses_and_skips_malformed() {
    let rows = parse_librispeech_tsv(FIXTURE);
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0].id, "a-1-0000");
    assert_eq!(rows[4].text, "NOPE");
}

#[test]
fn stratified_counts_cover_all_speakers_and_reproduce() {
    let rows = parse_librispeech_tsv(FIXTURE);
    let s1 = sample_stratified(&rows, 1, 20261101);
    let s2 = sample_stratified(&rows, 1, 20261101);
    assert_eq!(s1, s2, "same seed must reproduce");
    assert_eq!(s1.len(), 2);
    let mut spks: Vec<&str> = s1.iter().map(|r| r.id.split('-').next().unwrap()).collect();
    spks.sort();
    assert_eq!(spks, vec!["a", "b"]);
    // Short speakers contribute all their rows.
    let s3 = sample_stratified(&rows, 10, 7);
    assert_eq!(s3.len(), 5);
}
