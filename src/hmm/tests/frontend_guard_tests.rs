use hmm::{HmmSet, check_frontend};

const CMVN: &str = "hmm-fe2.cmvn-pop-floor1e-6";
const RAW: &str = "hmm-fe1.raw";

fn tiny_model() -> HmmSet {
    let phones = ["T", "AA"];
    let obs = vec![vec![vec![0.1, -0.2], vec![0.3, 0.4]]];
    HmmSet::flat_start(&phones, &obs, 0.01).expect("flat_start")
}

#[test]
fn guard_match_passes() {
    let id = Some(CMVN.to_string());
    check_frontend(&id, CMVN, "m.json").expect("match passes");
}

#[test]
fn guard_mismatch_refuses_and_names_both() {
    let id = Some(RAW.to_string());
    let err = check_frontend(&id, CMVN, "m.json").expect_err("mismatch must refuse");
    let msg = format!("{err:?}");
    assert!(
        msg.contains(RAW) && msg.contains(CMVN),
        "error must name both sides: {msg}"
    );
}

#[test]
fn guard_legacy_warns_and_passes() {
    check_frontend(&None, CMVN, "legacy.json").expect("legacy proceeds with warning");
}

#[test]
fn legacy_checkpoint_deserializes_to_none() {
    let mut m = tiny_model();
    m.frontend_id = Some(CMVN.to_string());
    let mut v: serde_json::Value = serde_json::to_value(&m).expect("to json");
    v.as_object_mut().expect("obj").remove("frontend_id");
    let back: HmmSet = serde_json::from_value(v).expect("legacy json loads");
    assert_eq!(back.frontend_id, None);
}

#[test]
fn stamped_id_roundtrips_through_text() {
    let mut m = tiny_model();
    m.frontend_id = Some(RAW.to_string());
    let path = std::env::temp_dir().join(format!("frontend_guard_{}.json", std::process::id()));
    m.save_text(&path).expect("save");
    let back = HmmSet::load_text(&path).expect("load");
    assert_eq!(back.frontend_id, Some(RAW.to_string()));
    let _ = std::fs::remove_file(&path);
}
