# Public-release checklist (lane J, bead hearsay-four-rw9q)

Scanned 2026-11-02 on `main@9357271` + this branch. Lane J proposes
keep/strip per item with reasons — **the owner decides**. Nothing here is
decided until the owner-decision list at the bottom is signed off.

Method: `git ls-files` enumeration, `scripts/personal_guard.sh`,
`scripts/check_tracked_artifacts.sh` allowlist review, secret-pattern grep
(`sk-*/ghp_*/gho_*`, `COURTLISTENER_API_TOKEN=`), `/home/` grep over tracked
sources, and `git log --all` history checks for personal/corpora paths.

## Per-item verdicts

### 1. Personal book text / LM — PROPOSE KEEP (manifest only)

- `data/DATA-1-book-manifest.json` tracks counts, sha256, and strip rules
  only; the PDF, extracted text, and derived LM live in
  `~/hearsay-personal/` outside the repo. No excerpts in any doc.
- `sh scripts/personal_guard.sh` → `PERSONAL-GUARD-OK`; history check
  (`git log --all -- '*owner-book*' '*hearsay-personal*'`) is empty.
- Reason: the manifest is provenance without content; it documents the
  personal-LM component without publishing a byte of the book.

### 2. Owner audio — PROPOSE KEEP (current state)

- No owner-voice recordings are tracked. `data/session/audio/` is gitignored
  (main checkout holds only a public SCOTUS argument MP3, not owner voice).
- Only tracked audio: the 4 allowlisted `data/demo/` fixtures (public-domain
  SCOTUS clips + synthetic `corrupt.mp3`), each with provenance in
  `data/demo/README.md`. No `.mp3/.wav/.flac` under `data/corpus/`.

### 3. `.env` / keys / secrets — PROPOSE STRIP (blocking)

- `.env` itself is gitignored and never committed (history check empty);
  `.env.example` holds only a `---` placeholder; `src/corpus` loads the token
  from env/`.env` with a `TOKEN_NEVER_HARDCODED` invariant.
- **BUT live secrets are tracked inside audit files:**
  - CourtListener API token `40b6035c…` (40 hex chars) in
    `docs/audit/transcripts/opencode-ses_ff82d2902ffeB47AD2RfaYPqOz.json`,
    `docs/audit/transcripts/opencode-ses_ffd6c5422ffep2RdSLp7h2C0jW.json`,
    and `docs/audit/openstate-export-20261031T153751Z.json`
    (user pasted it into chat asking it be kept out of git — the
    transcripts captured it anyway).
  - Cerebras API key `csk-vnc5…` in
    `docs/audit/transcripts/opencode-ses_ff966ff16ffeICRqgbyddkBUVf.json`
    (opencode config dump inside the transcript).
- Reason: these are the single blocking item. Strip the files (item 5) AND
  rotate both keys, since the values are already in git history.

### 4. Absolute `/home/` paths — PROPOSE REWRITE (non-blocking)

- ~20 tracked files contain `/home/michael/...`: mostly docs (lane-dir links
  in `D1-PARTITIONS.md`/`recording-protocol.md`, `Measured in lane …` lines,
  better-voice roots in `D0-DATA-INVENTORY.md`), plus hardcoded default
  constants in `src/runner/src/demo_transcribe.rs` (AM/LM/lexicon/wav-root).
- Reason: low secrecy risk (paths, not credentials) but unprofessional in
  public and the code constants break out-of-box use elsewhere. Docs-only
  lane cannot edit `src/`; rewrite is a follow-up bead.

### 5. OpenState export + transcripts (29 MB) — PROPOSE STRIP

- `docs/audit/openstate-export-20261031T153751Z.json` (5 MB) +
  `docs/audit/transcripts/*.json` (4 files, 24 MB) are tracked.
- Reasons: (a) they contain the live secrets from item 3; (b) 29 MB of
  session chatter bloats every clone; (c) internal agent transcripts are not
  release material. Keep the small audit `.md` summaries
  (`AUDIT-PACKAGE.md`, `SESSION-HISTORY.md`, cloud audit/rejoinder, refocus
  notes); move the JSON bulk to a private audit archive. `.gitignore`
  additions on this branch already ignore these paths going forward —
  removal of the tracked copies is the owner's `git rm`.

### 6. AGENTS.md handling — PROPOSE KEEP OUT (no action)

- `AGENTS.md` (and `CLAUDE.md`, `.claude/`, etc.) are gitignored and
  untracked; the file exists only in the owner's local checkout. Nothing to
  strip; the ignore rules stay so agent instructions never publish.

### 7. Better-voice / external-corpora references — PROPOSE KEEP (manifest only)

- `data/lexicon/manifest.json` records tier-1 identity + sha256 + counts; the
  4 MB tier-1 file and all LibriSpeech audio/LM text are gitignored.
  `data/SHA256SUMS.librispeech` / `data/SHA256SUMS.lm-text` are hash
  manifests only. History check for `data/librispeech/` is empty.
- Reason: same shape as item 1 — reproducible pointers, no redistributed
  content. Note: tier-1 source lives in the owner's separate better-voice
  checkout, so public users need a substitute (owner decision 5).

### 8. `models/si_base/model.json` — PROPOSE KEEP

- Tracked classical GMM means/weights (39-dim, human-readable JSON), not
  neural weights; `scripts/no_model_guard.sh` and
  `scripts/check_tracked_artifacts.sh` both permit it. Reason: it is the
  inspectable baseline the no-AI claim is about.

### 9. `dist/` release proofs — PROPOSE KEEP

- `dist/smoke.json`, `dist/no_model_guard.txt`, `dist/SHA256SUMS` are small,
  regenerable, secret-free proof artifacts. Reason: they let a judge verify
  the release without building.

### 10. Root docs + scripts — PROPOSE KEEP

- `HEARSAY-IV-*.md`, `RULE-SYSTEM-CONVERSION-INVENTORY.md`,
  `OPENSTATE-TRAJECTORY.md`, `script.txt`, `script-160.txt` (word-list
  prompt scripts), and `scripts/*` contain no PII, secrets, or personal
  text (email/phone grep over docs: no hits).

## Owner-decision list (all undecided — owner signs off)

- [ ] D1. License: accept proposed MIT (`LICENSE`, fill in `<OWNER-NAME>`)
      or choose Apache-2.0 (rewrite `LICENSE` accordingly).
- [ ] D2. Secrets: rotate the CourtListener token AND the Cerebras key
      (both values are in git history even after stripping).
- [ ] D3. Audit bulk: `git rm` the OpenState export + 4 transcripts and move
      them to a private archive (lane J proposes strip; owner executes).
- [ ] D4. `/home/` paths: approve a follow-up bead to relativize doc links
      and parameterize `demo_transcribe.rs` defaults (outside lane J paths).
- [ ] D5. Tier-1 lexicon: bless a public substitute for the better-voice
      tier-1 file (stock CMUdict-0.7b?) or document it as owner-local.
- [ ] D6. Release history: publish from current history (secrets stay in
      history forever — D2 rotation covers this) or squash/rewrite history
      before going public.
- [ ] D7. Confirm no owner-voice audio and no book text were ever staged in
      any lane branch that will merge before release (main history is clean;
      open lanes were not audited).

## Keep/strip summary (lane su00 completion)

Distilled from items 1–10 above. KEEP: `src/**`, `web/` demo pages,
`docs/` prose + results + video script, `scripts/` guards, `Cargo.toml`
files + lockfiles, `data/` manifests (`DATA-1-book-manifest.json`,
`lexicon/manifest.json`, `SHA256SUMS.*`), the 4 allowlisted
`data/demo/` fixtures, `models/si_base/model.json`, `dist/` proofs,
root docs. STRIP: live secrets (item 3, blocking) + the audit JSON
bulk (item 5, owner's `git rm`), `data/` corpora/audio/models (all
gitignored), `~/hearsay-personal/`, `target/`, `web/demo-out/`,
`.beads/` local state, sibling checkouts (referenced, never vendored).

## Secrets scan — rerun on the release commit (lane su00)

- [ ] G1 `bash scripts/no_model_guard.sh` → exit 0.
  - PRE-EXISTING (owner disposition 2026-11-03, leave open): guard is RED
    on `src/hmm/tests/phase_test_scoring_tests.rs` consuming
    `stt_transcript` outside `src/corpus` (from `f4fbdc0`); likewise
    `fabric survival_contract::d1` fails identically on pre-P0 main.
    Neither is caused by the A6 experiment work. Do not gate release on
    either; fix post-deadline.
- [ ] G2 `bash scripts/personal_guard.sh` → `PERSONAL-GUARD-OK`.
- [ ] G3 Key grep → no hits outside the D3 strip set (zero after D3):
  `grep -rEn 'AKIA[0-9A-Z]{16}|BEGIN (RSA |EC |OPENSSH )?PRIVATE[ ]KEY|api[_-]?key|ghp_[0-9A-Za-z]{36}|xox[bap]-|csk-[A-Za-z0-9]{20,}' src web scripts docs`
  (pattern is self-match-safe: brackets break the literal; the `csk-`
  length floor skips the redacted `csk-vnc5…` prefix quoted in item 3).
- [ ] G4 `git ls-files | grep -E '\.(wav|mp3|flac|onnx|safetensors|pt|pth|bin|arpa|lm\.bin)$'`
  → only the 4 allowlisted `data/demo/` fixtures; then
  `bash scripts/check_tracked_artifacts.sh` → exit 0.
- [ ] G5 `for p in target/x web/demo-out/x data/librispeech/x
  data/lm-text/x data/models/x sessions/canon/x; do
  git check-ignore -q "$p" || echo "NOT IGNORED: $p"; done` → no output
  (`data/`, `sessions/`, `dist/` are intentionally *partially* tracked —
  fixtures, manifests, proofs — so probe the ignored subpaths, not roots).

## Gate reruns — release commit, clean checkout, owner machine (lane su00)

- [ ] R1 `bash web/demo.sh` → `DEMO GREEN`, exit 0 (needs `data/`
  externals or `HEARSAY_LM`/`HEARSAY_TIER1`/`HEARSAY_TIER2`).
- [ ] R2 `bash web/tcv2-gate.sh` → exit 0.
- [ ] R3 `CARGO_TARGET_DIR=$HOME/.cargo/target-shared cargo test -p runner -p output`
  → all suites pass, 0 failed.
- [ ] R4 `CARGO_TARGET_DIR=$HOME/.cargo/target-shared cargo clippy -p runner --all-targets -- -D warnings`
  → no warnings.
- [ ] R5 `cargo fmt -p runner --check` → no diff.
- [ ] R6 `bash scripts/release.sh --target host` → `dist/smoke.json`,
  `dist/no_model_guard.txt`, `dist/SHA256SUMS` all written.

## Video / numbers consistency (lane su00)

- [ ] V1 Every `docs/video/SCRIPT.md` VO number matches its cited doc row
  (A6 arm-A table, ledger row 5, oracle K=10 row).
- [ ] V2 Shot F shows the arms-B/C BLOCKED verdict on screen (disclosed,
  not hidden).
- [ ] V3 `grep -c TBD docs/video/SCRIPT.md` → `0` (no unfilled inserts).

## Owner sign-off (lane su00)

- Lane J items decided: D1 D2 D3 D4 D5 D6 D7 (initial each above).
- Lane su00 reruns green: G1–G5 R1–R6 V1–V3 (initial each above).
- Release commit: ________________________________________
- Date (UTC): ________________________________________
- [ ] I ran every box above on the release commit and all are green.
  Owner signature: ________________________________________
