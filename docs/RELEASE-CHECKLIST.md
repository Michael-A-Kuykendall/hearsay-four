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
