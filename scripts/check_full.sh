#!/bin/bash
set -euo pipefail
# Repo root: this script lives in ./scripts/.
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$here"
./scripts/check.sh
./cargo.sh fmt
./cargo.sh fmt --check
