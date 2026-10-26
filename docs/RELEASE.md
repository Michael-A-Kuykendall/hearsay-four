# CLI release matrix (bead hearsay-four-dyy)

One-liner (host leg — the gate):

```bash
bash scripts/release.sh --target host
```

## What the script does

`scripts/release.sh` prints toolchain versions, builds `--release --bin
hearsay-demo` (offline), runs the human-readable smoke and the `--json` smoke
(bead hearsay-four-9gc) on the zero-config fixture `data/demo/sample.wav`
(bead hearsay-four-9hh), runs `scripts/no_model_guard.sh`, and archives the
proof into `dist/`:

- `dist/smoke.json` — JSON smoke result (must parse with nonzero
  `transcript.words`)
- `dist/no_model_guard.txt` — guard green output (model-free proof)
- `dist/SHA256SUMS` — sha256 checksums of the release binary + both files

The workspace-root exclude discipline is unchanged (only the `runner` bin is a
workspace member) and no runtime dependencies were added.

## Per-target commands (run verbatim)

| Leg     | Command                                             | Where           |
|---------|-----------------------------------------------------|-----------------|
| host    | `bash scripts/release.sh --target host`             | here (gate leg) |
| linux   | `bash scripts/release.sh --target linux`            | here / owner    |
| mac     | `bash scripts/release.sh --target mac`              | owner macOS session (`rustup target add aarch64-apple-darwin` first) |
| windows | `bash scripts/release.sh --target windows`          | owner session (`rustup target add x86_64-pc-windows-gnu` first) |

A bare Rust triple is also accepted: `bash scripts/release.sh --target
<triple>` (e.g. `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`,
`x86_64-pc-windows-gnu`).

## Cross-leg evidence

macOS/Windows toolchains are absent here, so those legs are documented-only in
this session. Their build + smoke + guard evidence is recorded under bead
hearsay-four-say during submission review.
