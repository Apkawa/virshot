#!/usr/bin/env bash
# gen-icons.sh: rasterize every SVG icon source into the icons crate PNG assets.
# Each crates/icons/assets/svg/NAME.svg becomes crates/icons/assets/png/NAME-<size>.png.
# Idempotent: re-running overwrites the PNGs from the current SVG sources.
# rsvg-convert (librsvg2-bin) is an external generator tool used here only — it is
# not a crate dependency and never part of the build or test path.
set -euo pipefail
# Repo root: this script lives in ./scripts/.
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$here"

SVG_DIR="crates/icons/assets/svg"
PNG_DIR="crates/icons/assets/png"
RASTER_W=128
RASTER_H=128

# The generator tool is required even when there is nothing to convert: a missing
# tool must not look like a successful run.
if ! command -v rsvg-convert >/dev/null 2>&1; then
  echo "gen-icons.sh: rsvg-convert not found — install librsvg2-bin" >&2
  exit 1
fi

if [ ! -d "$SVG_DIR" ]; then
  echo "gen-icons.sh: no SVG sources in $SVG_DIR — nothing to generate"
  exit 0
fi

mkdir -p "$PNG_DIR"

count=0
for src in "$SVG_DIR"/*.svg; do
  # With no match the glob stays literal, so skip it rather than feed it to the tool.
  if [ ! -f "$src" ]; then
    continue
  fi
  out="$PNG_DIR/$(basename "$src" .svg)-${RASTER_W}.png"
  if ! rsvg-convert -f png -w "$RASTER_W" -h "$RASTER_H" -o "$out" "$src"; then
    echo "gen-icons.sh: rsvg-convert failed on $src" >&2
    exit 1
  fi
  count=$((count + 1))
done

if [ "$count" -eq 0 ]; then
  echo "gen-icons.sh: no SVG sources in $SVG_DIR — nothing to generate"
  exit 0
fi

echo "gen-icons.sh: generated $count PNG asset(s) at ${RASTER_W}x${RASTER_H} into $PNG_DIR"
