#!/usr/bin/env bash
# cargo wrapper: pins the repo-local cache (CARGO_HOME=./.cache/cargo).
# Usage: ./cargo.sh <cargo args...>   (e.g. ./cargo.sh test, ./cargo.sh build --release --target x86_64-pc-windows-gnu)
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export CARGO_HOME="$here/.cache/cargo"
exec cargo "$@"
