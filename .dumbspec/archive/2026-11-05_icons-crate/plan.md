# 2026-11-05_icons-crate: implementation plan (иконки для virshot)

Spec — [spec.md](./spec.md); research — [research.md](./research.md).

**Format:** each stage = TDD (red test → green implementation → refactor) + a commit at the end.
This file is a **living progress journal** — update statuses as work proceeds.

Legend: `[ ]` not started · `[~]` in progress · `[x]` done.

---

## Stage 0 — Research + spec + plan

- [x] draft.md captured verbatim from the user request
- [x] refinement confirmed by user (research as-is)
- [x] research fan-out: tray glyph, window-icon API, egui texture API, crate conventions, rsvg-convert toolchain → research.md
- [x] spec.md v1 (full format, Created + Changes)
- [x] spec reviewed — open questions resolved into decisions, none blocking
- [x] plan.md written
- [x] execution confirmed by user; spec commit landed (`25f3f5b` pre-commit gate passed)

**Criterion:** all three artifacts exist in `current/2026-11-05_icons-crate/` and the plan has no blocking questions.
**Commit:** `docs(dumbspec): icons crate spec v1 + plan` — landed as `25f3f5b`

## Stage 1 — Crate scaffold `crates/icons`

- [x] failing test first: `crates/icons/src/lib.rs` inline tests for embedded PNG assets (`app_icon_png_is_embedded_png`, `warning_icon_png_is_embedded_png`); red proven: `-p icons` does not resolve (crate unwired) and `include_bytes!` targets missing
- [x] `crates/icons/Cargo.toml`: `name = "icons"`, `version = "0.1.0"`, `edition = "2024"`; deps `egui = "0.37"`, `image = { version = "0.25", default-features = false, features = ["png"] }`, `thiserror = "2"`; `[dev-dependencies] eframe` (mirror root block)
- [x] `src/lib.rs`: crate doc; `#![forbid(unsafe_code)]`; consts `APP_ICON_PNG` / `WARNING_ICON_PNG` via `include_bytes!`; `rgba(png) -> Result<(Vec<u8>, u32, u32), IconsError>` (decode via `image`); `icon_data(png) -> Result<egui::IconData, IconsError>`; integration tests `tests/icons.rs` (3 contract tests, green-proven in isolation; API note: image 0.25 encodes via `ImageEncoder::write_image`)
- [x] `src/error.rs`: `IconsError` thiserror enum (decode failure) + integration test `crates/icons/tests/error.rs` (contract pinned; green-proven in isolation while crate still red on assets)
- [x] placeholder PNG assets under `crates/icons/assets/png/` (tracked): transparent 128×128 RGBA via rsvg-convert from a scratch SVG; `./cargo.sh test -p icons` fully green (6 tests) with placeholders
- [x] root wiring: `icons = { path = "crates/icons" }` in `[dependencies]`, `"crates/icons"` appended to `[workspace] members` (cargo does NOT auto-discover; wiring mandatory). Red kept: `-p icons` resolves and fails on missing assets; check.sh fails at icons asset errors; Cargo.lock gains only the `icons` path-package (no new crates.io versions)
- [x] green: `./cargo.sh test -p icons` passes (6 tests); `./scripts/check.sh` passes (icons compiles as root dep, clippy clean). Independent acceptance: PASS 6/6

**Criterion:** `./cargo.sh test -p icons` green and `./scripts/check.sh` green with icons wired into workspace; `include_bytes!` assets tracked in git.
**Commit:** `feat(icons): crate scaffold with embedded PNG icon assets` — landed as `1e90f5a`

## Stage 2 — SVG sources + generation pipeline + two icons

- [x] `scripts/gen-icons.sh`: iterate `crates/icons/assets/svg/*.svg` → `rsvg-convert -f png -w 128 -h 128` → `crates/icons/assets/png/<name>-128.png`; idempotent (byte-identical reruns), fails loudly if `rsvg-convert` missing or a conversion fails; empty SVG dir → notice + exit 0
- [x] `crates/icons/assets/svg/app.svg`: camera glyph (viewfinder bump + body + lens ring + dark core) on transparent background, viewBox 128×128, palette延续 tray glyph (#505050/#c8c8c8/#181818); scratch raster → RGBA, corners alpha 0, opaque body present
- [x] `crates/icons/assets/svg/warning.svg`: yellow rounded triangle (#F0B429 fill, #181818 border via stroke-linejoin round), black exclamation bar + dot centered at x=64, transparent background; scratch raster RGBA, corners alpha 0, yellow 2605 / black 2666 opaque px
- [x] failing tests first (integration `tests/icons.rs`): `app_icon_asset_decodes_128x128`, `warning_asset_decodes_128x128`, `app_icon_asset_has_opaque_body_and_transparent_corners`, `warning_asset_has_yellow_and_black_marks` — red proven against placeholders (opaque-body + yellow/black fail; dims green by construction)
- [x] run `./scripts/gen-icons.sh`, regenerate PNG assets (placeholders → real art: app 2625 B, warning 3149 B), tests green (10/10), idempotent rerun (sha256 identical)
- [x] asset spot-check: `file crates/icons/assets/png/*.png` → `PNG image data, 128 x 128, 8-bit/color RGBA`

**Criterion:** `./scripts/gen-icons.sh` regenerates tracked 128×128 RGBA PNGs from SVGs; lib decode tests green; `file` confirms size/format.
**Commit:** `feat(icons): svg sources + rsvg-convert generation + app/warning icons`

## Stage 3 — Example gallery `crates/icons/examples`

- [x] `crates/icons/examples/icons_gallery.rs`: eframe window listing every embedded icon (texture per icon via `icons::rgba` + `ctx.load_texture` on first frame; `painter().image` row + name label; self-terminate after 10 frames via `ViewportCommand::Close` — egui 0.37 has no `Context::terminate`); head-doc + Wayland caveat (`env -u WAYLAND_DISPLAY` required under sandbox)
- [x] headless verification: `env -u WAYLAND_DISPLAY xvfb-run -a … ./cargo.sh run --example icons_gallery -p icons` exits 0 on its own (self-terminate at frame 10), no panic/backtrace in ./.tmp/icons_gallery_verify.log
- [x] `./cargo.sh clippy -p icons --all-targets -- -D warnings` clean (examples included)

**Criterion:** example window runs headless under xvfb showing app + warning icons, clippy clean on all targets.
**Commit:** `feat(icons): eframe example gallery of all icons`

## Stage 4 — Integration into virshot

- [x] `src/tray.rs`: `camera_glyph_rgba()` + fill helpers gone; private `app_icon_rgba()` decodes `icons::APP_ICON_PNG`, `TrayController::new` builds `Icon::from_rgba` from it (IconsError → `Error::TrayUnavailable`, main.rs logs & continues); tray.tests.rs adapted (3 asset tests) — TDD red→green, 249 passed, check.sh green
- [x] `src/gui.rs`: private `app_window_icon()` → `icons::icon_data(APP_ICON_PNG)` (IconsError → tracing::warn + iconless window); viewport gets `.with_icon(Arc<IconData>)` AT creation (Windows ICON_BIG captured then); gui.tests.rs sibling +2 asset tests — TDD red→green, 251 passed, check.sh green
- [x] `src/gui.rs`: warning status rows paint `WARNING_ICON_PNG` texture (ColorImage via `warning_color_image()` → `load_texture` once at window creation, `painter().image` into a fixed 16px slot; ASCII `"!"` fallback + warn on decode error; info `"i"` untouched); gui.tests.rs +2 tests — TDD red→green, 253 passed, check.sh green
- [x] GUI verification: `./scripts/ui-verify.sh all --wm` green — tray `IconPixmap 128x128 bytes=65536` (converted PNG pixel-identical to app-128.png: corner alpha 0, gray80 7695 px), settings window `_NET_WM_ICON` present `Icon (128 x 128)` under Xvfb+openbox; harness assertions adapted (was 32×32 glyph); warning row = texture+ASCII-fallback pinned by unit tests (visual in settings-window shot not separately asserted)
- [x] `./scripts/check.sh` green

**Criterion:** tray uses the app-icon asset (glyph fn gone), settings window carries the app icon, warning status renders the triangle icon; check.sh green; tray tests pass.
**Commit:** `feat(virshot): app icon for tray and window, warning icon in gui`

## Stage 5 — Final gates

- [x] `./cargo.sh test --workspace` green (exit 0: virshot 253 + e2e 9, icons/overlay/toast/gamepad targets ok)
- [x] `./cargo.sh clippy --workspace --all-targets -- -D warnings` green (exit 0, icons compiled)
- [x] Windows gate: `./cargo.sh test --no-run --target x86_64-pc-windows-gnu` compiles (exit 0, exe binaries built)
- [x] `./scripts/check_full.sh` green (fmt + fmt --check across members)

**Criterion:** all four gate commands exit 0 — done (stage-boundary run, logs ./.tmp/stage5_*.log).
**Commit:** `chore(icons): workspace gates green`
