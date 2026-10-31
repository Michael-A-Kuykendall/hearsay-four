//! DATA-1 privacy gate: the personal guard must pass on the tracked tree.
//! (bead hearsay-four-79f--data-intake-corpora). The guard logic lives in
//! scripts/personal_guard.sh; this test wires it into `cargo test -p corpus`.
use std::path::PathBuf;
use std::process::Command;

#[test]
fn personal_guard_passes_on_tracked_tree() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .ancestors()
        .nth(2)
        .expect("workspace root above src/corpus")
        .to_path_buf();
    let script = root.join("scripts/personal_guard.sh");
    assert!(
        script.exists(),
        "missing guard script: {}",
        script.display()
    );
    let out = Command::new("sh")
        .arg(&script)
        .current_dir(&root)
        .output()
        .expect("run personal guard");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "personal guard failed:\n{stdout}\n{stderr}"
    );
    assert!(
        stdout.contains("PERSONAL-GUARD-OK"),
        "guard output:\n{stdout}\n{stderr}"
    );
}
