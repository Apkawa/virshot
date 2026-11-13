#!/usr/bin/env bash
# Release helper driven by git-cliff (a dev *tool*, pinned 2.14.2 — not a crate dep).
# ZeroVer: `major` is refused; cliff.toml pins the bump rules + `initial_tag = "v0.1.0"`.
set -euo pipefail

# Repo root: this script lives in ./scripts/.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
CFG="$ROOT/cliff.toml"

err() {
  echo "release: $1" >&2
  exit 1
}

usage() {
  echo "usage: scripts/release.sh <changelog|minor|patch>" >&2
  echo "       (major is forbidden — ZeroVer keeps major=0)" >&2
  exit 1
}

# git-cliff: PATH first, then the repo-local cargo cache (`./cargo.sh install`).
resolve_cliff() {
  if command -v git-cliff >/dev/null 2>&1; then
    CLIFF="git-cliff"
  elif [ -x "$ROOT/.cache/cargo/bin/git-cliff" ]; then
    CLIFF="$ROOT/.cache/cargo/bin/git-cliff"
  else
    err "git-cliff not found — install it, or ./cargo.sh install git-cliff --version 2.14.2 --no-default-features"
  fi
}

# Rewrite the `version = "..."` line inside [package] only — dep tables elsewhere
# have inline `version =` keys that must stay untouched; other lines print verbatim.
# The error is about a MISSING version line (awk exit status), not about an
# unchanged file: the first release bumps nothing — git-cliff reports the
# `initial_tag` verbatim when no tags exist yet, so writing the same version back
# is a legitimate no-op.
rewrite_cargo_version() { # $1: bare version, e.g. 0.2.0
  local tmp="$ROOT/Cargo.toml.tmp.$$"
  if ! awk -v ver="$1" '
    /^[[:space:]]*\[/ { inpkg = ($0 ~ /^[[:space:]]*\[package\][[:space:]]*$/) }
    inpkg && /^version[[:space:]]*=/ { print "version = \"" ver "\""; found = 1; next }
    { print }
    END { exit !found }
  ' "$ROOT/Cargo.toml" > "$tmp"; then
    rm -f "$tmp"
    err "no version line found in the [package] section of Cargo.toml"
  fi
  mv "$tmp" "$ROOT/Cargo.toml"
}

if [ "$#" -ne 1 ]; then
  usage
fi

case "$1" in
  changelog)
    resolve_cliff
    "$CLIFF" --config "$CFG" -o "$ROOT/CHANGELOG.md"
    echo "CHANGELOG.md regenerated (no commit, no tag)"
    ;;
  major)
    err "major bumps are forbidden — ZeroVer keeps major=0; bump minor instead when ready"
    ;;
  minor|patch)
    resolve_cliff
    # Guard 1: uncommitted work would silently land inside the release commit.
    if [ -n "$(git status --porcelain)" ]; then
      err "working tree is dirty — commit or stash first"
    fi
    # Guard 2: git-cliff prints the bumped version on stdout (INFO goes to stderr).
    VER="$("$CLIFF" --config "$CFG" --bumped-version --bump "$1" 2>/dev/null)" || VER=""
    if [ -z "$VER" ]; then
      err "git-cliff produced no bumped version for '$1'"
    fi
    # Newest version tag (git 2.43 needs -l to accept a tag pattern).
    last="$(git tag --sort=-v:refname -l 'v[0-9]*' | head -n 1)"
    if [ "$VER" = "$last" ]; then
      err "nothing to release — no unreleased commits since $last"
    fi
    bare="${VER#v}"
    rewrite_cargo_version "$bare"
    # VER is future by construction (equal-to-last was rejected), so the tag is new.
    "$CLIFF" --config "$CFG" --tag "$VER" -o "$ROOT/CHANGELOG.md"
    git add Cargo.toml CHANGELOG.md
    git commit -m "chore(release): prepare for $VER"  # plain commit: hooks must run
    git tag -a "$VER" -m "Release $VER"
    echo "released $VER: Cargo.toml version $bare, CHANGELOG.md updated, tag $VER created"
    ;;
  *)
    usage
    ;;
esac
