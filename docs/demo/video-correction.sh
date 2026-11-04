#!/bin/bash
# Screen-recordable correction demo (shot F): one sentence, live decode ->
# why -> correct -> re-decode -> undo (byte-identical), then the 29/29 row.
# Same binary and models as correction-demo.sh; HEARSAY_DEMO_VIDEO=1 only
# trims output (one utterance, readable why line, no collateral report).
set -u
cd "$(dirname "$0")/../.." || exit 1
cargo build --release -p runner --bin hearsay-correct -q -j4 || exit 1
clear
HEARSAY_DEMO_VIDEO=1 bash docs/demo/correction-demo.sh 2>&1 | grep -v '^\s*Finished\|^hearsay-correct demo\|^base vocab'
echo
grep -m1 '29/29' docs/results/NUMBER-LEDGER.md
