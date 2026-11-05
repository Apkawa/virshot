#!/bin/bash
set -euo pipefail
# Repo root: this script lives in ./scripts/.
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$here"
./cargo.sh check
if [ "${SKIP_E2E:-0}" = "1" ]; then
  # Commit-time gate: the `tests/e2e` target drives the built binary, which can
  # park forever on a host without a usable display (monitor enumeration blocks
  # inside the capture backend), so the commit gate runs the unit tests only.
  # The e2e target is still *compiled* — its tests are filtered by the area
  # module prefix (`cli_headless::`), not dropped as a target, so compile drift
  # in tests/e2e still fails the gate. Full run: `./cargo.sh test`.
  ./cargo.sh test -- --skip 'cli_headless::'
else
  ./cargo.sh test
fi
./cargo.sh clippy -- -D warnings
