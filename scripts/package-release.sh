#!/usr/bin/env bash
# Packaging helper: already-built release binaries -> distributable archives +
# release.yaml metadata (full download URL, sha512, size). Called from GitHub
# Actions (ubuntu-latest package job) and runnable locally. Needs zip, tar, openssl.
# The version string is passed in by the caller; the script never parses Cargo.toml.
set -euo pipefail

# Repo root: this script lives in ./scripts/ (relative --out/--binaries resolve here).
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

err() {
  echo "package-release: $1" >&2
  exit 1
}

usage() {
  echo "usage: scripts/package-release.sh --binaries DIR --version VER --kind release|nightly [--sha SHORTSHA] --repo OWNER/REPO --tag TAG [--out DIR]" >&2
  exit 1
}

BINARIES=""
VERSION=""
KIND=""
SHA=""
REPO=""
TAG=""
OUT="dist"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --binaries) [ "$#" -ge 2 ] || usage; BINARIES="$2"; shift 2 ;;
    --version)  [ "$#" -ge 2 ] || usage; VERSION="$2"; shift 2 ;;
    --kind)     [ "$#" -ge 2 ] || usage; KIND="$2"; shift 2 ;;
    --sha)      [ "$#" -ge 2 ] || usage; SHA="$2"; shift 2 ;;
    --repo)     [ "$#" -ge 2 ] || usage; REPO="$2"; shift 2 ;;
    --tag)      [ "$#" -ge 2 ] || usage; TAG="$2"; shift 2 ;;
    --out)      [ "$#" -ge 2 ] || usage; OUT="$2"; shift 2 ;;
    *)          usage ;;
  esac
done

if [ -z "$BINARIES" ] || [ -z "$VERSION" ] || [ -z "$KIND" ] || [ -z "$REPO" ] || [ -z "$TAG" ]; then
  err "missing required option(s): --binaries, --version, --kind, --repo, --tag"
fi
case "$KIND" in
  release|nightly) ;;
  *) err "--kind must be release or nightly (got '$KIND')" ;;
esac
if [ "$KIND" = nightly ] && [ -z "$SHA" ]; then
  err "--sha is required for nightly builds"
fi
# Nightly passes its short sha; a release falls back to the current HEAD so
# local runs work without an explicit --sha.
if [ -z "$SHA" ]; then
  SHA="$(git rev-parse --short HEAD 2>/dev/null)" || err "no --sha given and git HEAD is not available"
fi

WIN_BIN="$BINARIES/win-x64/virshot.exe"
LINUX_BIN="$BINARIES/linux-x64/virshot"
for bin in "$WIN_BIN" "$LINUX_BIN"; do
  [ -f "$bin" ] || err "missing binary: $bin"
done

# Artifact names: release = virshot-<ver>-<target>; nightly inserts -nightly-<shortsha>.
base="virshot-$VERSION"
if [ "$KIND" = nightly ]; then
  base="$base-nightly-$SHA"
fi
ZIP="$base-win-x64.zip"
TARBALL="$base-linux-x64.tar.gz"

mkdir -p "$OUT"

# Base64 of the raw 64-byte sha512 digest (88 chars), as release.yaml consumers expect.
sha512_b64() {
  openssl dgst -sha512 -binary "$1" | openssl base64 -A
}

# Archives hold the binary only: zip -j strips the path, tar -C packs just the
# file name (no directory entries). rm -f first: zip appends to an existing
# archive, so a rerun into the same --out must start clean.
rm -f "$OUT/$ZIP"
zip -j "$OUT/$ZIP" "$WIN_BIN" >/dev/null
tar -czf "$OUT/$TARBALL" -C "$BINARIES/linux-x64" virshot

releaseDate="$(date -u +'%Y-%m-%dT%H:%M:%S.%3NZ')"
url_base="https://github.com/$REPO/releases/download/$TAG"
win_sha="$(sha512_b64 "$OUT/$ZIP")"
linux_sha="$(sha512_b64 "$OUT/$TARBALL")"
win_size="$(stat -c %s "$OUT/$ZIP")"
linux_size="$(stat -c %s "$OUT/$TARBALL")"

{
  echo "version: $VERSION"
  echo "releaseDate: '$releaseDate'"
  echo "commit_sha: $SHA"
  echo "files:"
  echo "  - url: $url_base/$ZIP"
  echo "    type: x86_64-pc-windows-msvc"
  echo "    sha512: $win_sha"
  echo "    size: $win_size"
  echo "  - url: $url_base/$TARBALL"
  echo "    type: x86_64-unknown-linux-gnu"
  echo "    sha512: $linux_sha"
  echo "    size: $linux_size"
} > "$OUT/release.yaml"

echo "packed $ZIP ($win_size bytes)"
echo "packed $TARBALL ($linux_size bytes)"
