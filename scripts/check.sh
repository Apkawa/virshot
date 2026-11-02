#!/bin/bash
set -euo pipefail
# Repo root: this script lives in ./scripts/.
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$here"
./cargo.sh check
./cargo.sh test
./cargo.sh clippy -- -D warnings
