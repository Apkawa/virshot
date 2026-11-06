# 2026-11-06_i18n-translations: research notes (English)

Superficial context collection only: dependencies, files, fixed constraints, existing solutions.
Findings appended incrementally as discovered.

## Findings — R1: `rust-i18n` crate facts (verified, subagent report)

- Latest release **4.2.4** (published 2026-09-29), license MIT, published crate edition
  2021, no declared MSRV (`rust_version: null`). Generated code uses
  `std::sync::LazyLock` (≥1.80) — repo toolchain 1.99.1 is fine.
  Source: https://crates.io/api/v1/crates/rust-i18n
- Features: none by default; `load-path` → `rust-i18n-support/codegen`,
  `log-miss-tr` → `rust-i18n-macro/log-miss-tr`. `log-miss-tr` requires the **app**
  crate to depend on `log` (the macro crate declares no `log` dep).
- The init macro is **`i18n!`** — there is **no `i18n_init!`** in 2.x/3.x/4.x
  (docs.rs item lists checked). Attributes: `fallback` (string or array),
  `backend`, `metadata`, `minify_key`, `minify_key_len`, `minify_key_prefix`,
  `minify_key_thresh`.
- **One-file `_version: 2`** is a plain data file (`.yml`/`.yaml`/`.json`/`.toml`)
  under `<CARGO_MANIFEST_DIR>/locales/`; filename is arbitrary because the locale
  is a **nested key** per entry, not the filename. Keys may contain dots; nested
  objects flatten into dotted keys. Values are unquoted plain YAML scalars.
  Verbatim shape:

  ```yml
  _version: 2
  hello:
    en: Hello world
    zh-CN: 你好世界
  messages.hello:
    en: Hello, %{name}
    zh-CN: 你好，%{name}
  ```

  Init: `i18n!("locales")`, `i18n!("locales", fallback = "en")`,
  `i18n!("locales", fallback = ["en", "es"])`, `i18n!()`.
- Runtime locale API: `rust_i18n::set_locale(&str)` (process-wide `AtomicStr`,
  thread-safe), `rust_i18n::locale() -> impl Deref<Target = str>` (use `&*locale()`),
  macros `t!`, `tkv!`, `available_locales!()`, `extend!(crate_name)`. There is **no
  public `tr!`**. Locale strings are exact/case-sensitive (`"zh-CN"`, `"ZH-cn"` misses).
  `t!` returns `Cow<'static, str>` (upstream uses `.into_owned()`).
- **Missing key never panics and never returns empty**: it falls back to the *input
  key text itself* (`t!("some.key")` → `"some.key"`); chain is current locale →
  shortened locale (`zh-Hant-CN-x1` → … → `zh`) → explicit `fallback` list → miss.
  **No automatic fallback to `en`** unless configured → we must pass
  `fallback = "en"` to `i18n!`.
- Translations are embedded **at build time** from `${CARGO_MANIFEST_DIR}/locales`
  (glob `**/*.{yml,yaml,json,toml}`; docs.rs page is stale and says `**/*.yml`).
  No disk reads at runtime; an empty/missing `locales/` compiles to an empty catalog.
  `[package.metadata.i18n] default-locale` **cannot force the startup locale**
  (the generated branch only applies when the current locale already equals it);
  startup locale is `"en"` and the app must call `set_locale()` explicitly.
- **Catalog is per crate, locale is process-wide.** `i18n!` emits the backend into the
  *calling* crate and `t!` expands to `crate::_rust_i18n_t!`, so `crates/toast` and
  `crates/overlay` each need their own `i18n!` + own `locales/` dir (two calls in the
  same crate collide). App-level overrides for a dependency go via
  `rust_i18n::extend!(toast)` at startup (namespace = crate name as referenced by the
  app), with translations nested under that namespace in the app file; calling
  `extend!` twice for the same namespace **panics**. Generated code hard-codes the
  `rust_i18n::` path, so an aliased/renamed rust-i18n dep would break it.
  Reference: upstream `examples/extend-crate`.
- Dependency chain of 4.2.4: `rust-i18n-macro`, `rust-i18n-support`, `smallvec`;
  macro crate pulls `glob`, `proc-macro2`, `quote`, `syn`, `serde`, `serde_json`
  (host-side codegen); support: `arc-swap`, `base62`, `foldhash`, `siphasher`,
  `triomphe` (+ codegen-only `globwalk`, `itertools`, `normpath`, `serde-saphyr`,
  `serde_json`, `toml`). Pure Rust, no native/C libs → Windows-gnu cross-build
  plausible (not compile-verified). Crate has internal `unsafe`
  (`str::from_utf8_unchecked`) — outside the repo's `forbid(unsafe_code)` rule.
- Uncertainties left: no MSRV declared upstream; whether codegen deps are linked into
  the shipped binary was not verified by building; `default_locale` semantics beyond
  source reading; Windows-gnu build not exercised.

## Findings — R2: user-visible string inventory (verified, subagent report)

- Settings window is a **3-tab layout**, no collapsing sections. Tab labels verbatim:
  `"General"`, `"Hotkeys"`, `"Overlay"` (`src/gui.rs:532-534`, painted at
  `src/gui.rs:2172-2178`). Window title `"virshot — Settings"` (`src/gui.rs:2298`),
  eframe app name `"virshot"` (`src/gui.rs:2327`). Bottom bar: `"Unsaved changes"`,
  `"Cancel"`, `"OK"`, `"Apply"` (`src/gui.rs:2108-2148`).
- General tab is `SettingsApp::general_tab` (`src/gui.rs:1627`): Save path + Browse,
  full-screen/window templates, `Placeholder reference` collapsing header
  (`src/gui.rs:1654`) with 11 token descriptions from `src/template.rs:22-35`,
  Format ComboBox (`src/gui.rs:1666-1672`), JPEG/WebP quality DragValues
  (`src/gui.rs:1674-1692`). **Natural spot for the language dropdown**: inside
  `general_tab` right after the Format ComboBox (`src/gui.rs:1673`).
- Inventory size: **≈95 translatable strings** — chrome/tabs/bottom bar 9, General 25,
  Hotkeys 21, Gamepad pad field 5, Overlay tab 12, toast chip texts 15, overlay window
  title 1, tray 4, CLI 19. Families **intentionally not translated**: key-name table
  (`src/gui.rs:333-442`), `MOD_NAMES` (`src/gui.rs:446`), pad notation
  (`src/gamepad.rs:137-142`), combo `+` separator, placeholder token notation
  (`src/gui.rs:1660`), format values (`png`/`jpg`/`webp`), `"!"`/`"i"` icon glyphs,
  stdout saved-path data.
- Interpolated strings need `%{...}` placeholders: toast chip `"Saved: {}"`
  (`src/capture.rs:217-218`), duplicate tooltips (`src/gui.rs:759-774`), registration
  failure tooltips (`src/hotkeys.rs:804-808`), capture/template error chips
  (`src/capture.rs:80-424`, `src/template.rs:55-65`), CLI stderr messages
  (`src/main.rs:191-338`, `src/instance.rs:24-39`, `src/logging.rs:42-48`).
- Toast chip text is painted by `crates/toast/src/chip.rs:68-74`, but the **text itself
  is produced in the root crate** (`src/capture.rs`, `src/gui.rs` test chip) → the
  catalog for those keys lives in the root crate. `crates/overlay` has only one
  user-visible string (`"virshot overlay"` window title, `crates/overlay/src/gl_boot.rs:198`,
  decoration-less/invisible in practice) → its own `i18n!` is not worth adding.
- **No locale/i18n code exists today** (`rg` for locale/language/i18n found nothing in
  code); all UI text is inline `&'static str` or `format!`.
- **Scope conflict with project spec:** `docs/spec.md:101` says i18n is out of MVP
  ("UI в MVP только на английском", ключи конфига и CLI — английские) and
  `docs/spec.md:360` says "Язык UI — английский"; `src/tray.rs:53` carries a comment
  quoting that note. This task overrides it → `docs/spec.md` must be updated as part
  of the work.

## Findings — R3: config persistence, startup wiring, apply flow, test surface

- **Config model** (`src/config.rs:513-521`): `Settings { general, controls, overlay }`,
  every struct `#[serde(default)]` (container-level) + manual `impl Default`.
  `General` (`:398-408`) serialized keys: `save_path`, `file_full_screen`,
  `file_window`, `format`, `jpeg_quality`, `webp_quality`; `Format` enum uses
  `#[serde(rename = "png"|"jpg"|"webp")]`; `OverlayPosition` serializes variant names
  verbatim. **No `deny_unknown_fields` anywhere** → old/unknown keys are ignored, so a
  new `language` key loads fine on old configs and old configs load fine on the new build.
- **Invalid value semantics:** `load` → `Error::Io`/`Error::Parse`; `load_or_default`
  (`:117-127`) logs and returns **whole-config defaults** on any parse error. So a
  **strict enum** `language` (`language = "german"`) resets *everything* (hotkeys, save
  path, overlay) to defaults. A lenient field (`String` + normalization → `"en"`) keeps
  the rest of the config. Decision left to the spec.
- **Writes:** only `config::save` (`:167-171`, `toml::to_string_pretty` + `fs::write`,
  non-atomic, user formatting lost) from first-run `create_default_file` (`:157`) and
  GUI `apply()` (`src/gui.rs:1338`). Config path: `default_config_path()` (`:344-361`)
  — Windows portable/`%APPDATA%`, Linux `APPDATA`→`XDG_CONFIG_HOME`→`$HOME/.config`,
  always `virshot/config.toml`; CLI default `main.rs:120`.
- **Apply flow:** `SettingsApp` holds `applied`/`draft`; dirty = `draft != applied`
  (`gui.rs:1063`); `apply()` (`gui.rs:1327-1361`) saves the full TOML, sets `applied`,
  then `controller.apply(draft.clone())` → shared mailbox `Arc<Mutex<Settings>>`
  (`main.rs:356`, written at `hotkeys.rs:486`, cloned per trigger/pump); capture takes an
  owned `Settings` clone. So a `language` field inside `Settings` propagates to listener,
  pad pump, and capture automatically; GUI rebuilds text every frame.
- **Startup order (`run_resident`, `main.rs:325-424`):** `config::load_or_create` (:326)
  → instance guard (:333) → mailbox (:356) → `hotkeys::HotkeysController::start` (:361)
  → gamepad pump (:383) → **`tray::TrayController::new()` (:392) — menu labels are built
  inside this call** → `gui::run` (:404). Correct insertion point for `set_locale()` is
  right after config load (:326) and before :392. Headless `run_shot` loads config at
  :227; if CLI text is localized it needs the same call there.
- **Existing tests pin English text** and must be rewritten as default-locale assertions:
  `src/gui.tests.rs` — `tab_labels_are_english` (:923), `row_labels_are_english_per_spec`
  (:930), `format_labels_match_config_names` (:937), `bind_hint_mentions_escape_and_backspace`
  (:1015, asserts `BIND_HINT.is_ascii()`), `duplicate_save_tooltip_explains_why_saving_is_blocked`
  (:871, `is_ascii`), `overlay_test_message_matches_real_save_success_wording` (:1404), pad/bind
  label tests; `src/tray.tests.rs:89 menu_labels_match_spec_section_4_5`. Config tests
  (`src/config.tests.rs`, 683 lines) cover defaults/partial TOML/round-trip — the new
  `language` field needs `language_default_is_english`, a partial-TOML default fill, and a
  round-trip test following the existing fixtures.
- **Manifests:** root `Cargo.toml` deps verbatim (xcap 0.9.8, image 0.25 trimmed,
  chrono 0.4.45, thiserror 2, eframe 0.37.0, serde 1.0.229, toml 1.1.8, tracing 0.1.44,
  tracing-subscriber 0.3.23, clap 4.6.7, tray-icon 0.26.1 ksni, egui 0.37.0,
  global-hotkey 0.9.0, rfd 0.17.2, path deps `overlay`, `toast`, `icons`; cfg(windows)
  `gamepad`; dev `rusty-hook`). Workspace members: `[".", crates/overlay, crates/toast,
  crates/gamepad, crates/icons]`, exclude `hidpad-poc`. **No `[package.metadata.i18n]`,
  no `locales/` dir anywhere**, no i18n dependency; `icu_*` in `Cargo.lock` are transitive
  only (via `idna`), not usable machinery.
- **Spec touchpoints to update in `docs/spec.md`:** §2 out-of-scope i18n bullet (:101-102),
  §4.4 "Язык UI — английский" (:360), §4.3 toast wording (:321), §4.5 tray menu labels
  (:488), §7 config schema `[general]` (:547-577) + corrupt-config note (:541),
  §10 testing (:708 ff). **§5 binary budget <10 MB** (:496) — rust-i18n embeds catalogs
  statically (pure Rust, no native libs), so the size impact must be checked against this
  budget at the release-build step.
- Design facts the spec must carry: language must be applied **after config load and
  before tray/GUI construction**; tray menu labels are built once at construction →
  changing the language at Apply needs a tray menu rebuild; toast chip text originates in
  the root crate, so its catalog lives in the root `locales/` file; `crates/overlay`
  has one invisible window-title string → no per-crate i18n needed there.

## Findings — R4 (execution, Stage 1 red tests): macro wiring constraints

- `rust_i18n::t!` expands to `crate::_rust_i18n_t!` (`rust-i18n-4.2.4/src/lib.rs:382`),
  so the **init macro must run in the crate root** (`src/main.rs`), not in a sibling
  module; otherwise every `t!` call is `E0433: cannot find _rust_i18n_t in the crate root`.
- `minify_key` must stay **off** (default) — hashing literal keys breaks the pinned
  key→text contract the tests assert.
- `fallback = "en"` is required for miss-fallback to English (`_RUST_I18N_FALLBACK_LOCALE`,
  `rust-i18n-macro-4.2.4/src/lib.rs:536-541`); without it a miss returns the dotted key
  text (`"es.general.tab_general"`).
- Default locale is `"en"` from the `CURRENT_LOCALE` static (`rust-i18n-4.2.4/src/lib.rs:21`)
  — `locale_default_is_english` passes without init; `[package.metadata.i18n]` is not needed.
- Locale access is process-wide but **cached per thread**, so locale-touching tests must be
  serialized (`src/i18n.tests.rs` uses a static `LOCALE_LOCK` mutex) and must restore
  `"en"` before asserting.
- **Key mapping pinned by red tests** (English literal → source location):
  `general.tab_general`/`tab_hotkeys`/`tab_overlay` (gui.rs:532-534),
  `general.save_path_label` (:1631), `general.full_screen_label` (:1643),
  `general.window_label` (:1646), `general.placeholder_reference_header` (:1654),
  `general.format_label` (:1666), `general.jpeg_quality_label` (:1679),
  `general.webp_quality_label` — full label "WebP quality (WebP is lossless in MVP —
  quality not applied yet)" (:1687), `general.template_title_default` = "fullscreen"
  (template.rs:45), `bottom.cancel/ok/apply` (:2125-2129), `bottom.unsaved_changes`
  (:2146), `bottom.window_title` = "virshot — Settings" (:2298), `hotkey.bind_hint` (:493),
  `hotkey.unassigned` (:498), `hotkey.pad_label` (:476), `hotkey.already_registered`
  (hotkeys.rs:805, `%{combo}`), `hotkey.registration_failed` (hotkeys.rs:808, `%{reason}`),
  `overlay.enabled_label` (:1896), `overlay.duration_label` (:1916),
  `overlay.position_label` (:1919), `overlay.on_all_monitors_label` (:1936),
  `overlay.test_button` (:1898), `toast.saved` = "Saved: %{file_name}" (capture.rs:217),
  `toast.save_failed` = "failed to save %{path}: %{reason}" (capture.rs:95 thiserror
  message), `tray.take_screenshot/settings/exit` (tray.rs:54-56), `tray.tooltip` (:64).
- **`overlay.test_message` has no separate key** — it is `toast.saved` applied to the
  fixed `OVERLAY_TEST_FILENAME` ("test.png") through `gui::overlay_test_message`
  (gui.rs:255/272); the test pins that equality.
- Existing UI literals still to migrate (English text exists in source, no key yet):
  "Press a button…" (gui.rs:463), pad capture hint (gui.rs:469), gamepad trigger hint
  (gui.rs:473), "(gamepad — Windows only)" (gui.rs:482), overlay test enable hint
  (gui.rs:261). Keys with **no** source literal (`general.language_label`, `hotkey.header`,
  `hotkey.current_key`, `hotkey.mods_label`, `overlay.header`, `paths.home_hint`,
  `gamepad.*`) are out of the catalog until an English literal is introduced —
  `general.language_label` = "Language" is the only genuinely new label (dropdown).




## R5 — Catalog rebuild hazard (found during Stage 3, fixed before the commit)

rust-i18n 4.2.4 reads `./locales/` during **macro expansion** (`i18n!` at the crate
root), and its build script emits `rerun-if-changed` scoped to the rust-i18n crate's
own directory, so rustc's dep-info for `virshot` never lists `locales/app.yml`:
editing a translation and rebuilding ships **stale strings** (Stage 3 needed a manual
`touch src/main.rs` to see the new Russian text). Fix: a tracked compile edge —
`const I18N_CATALOG_SOURCE: &str = include_str!("../locales/app.yml")` at the crate
root (`src/main.rs:72-78`) with a startup-only `tracing::debug!` consumer
(`src/main.rs:348-351`) so the const is a real use rather than dead code. Verified:
appending one comment line to the catalog re-checks `virshot`, a no-op run does not;
`catalog_source_is_embedded_as_a_compile_dependency` pins that the embedded copy equals
the on-disk file byte-for-byte.

## R6 — Release size after i18n

Measured 2026-11-06 on the Linux cross-build host (`LANG=C.UTF-8`, x86_64), target
`x86_64-pc-windows-gnu`, release profile `opt-level="z"`, `lto="thin"`, `strip=true`
(`Cargo.toml:77-80`).

- Binary: `target/x86_64-pc-windows-gnu/release/virshot.exe` — 8352768 bytes
  (`ls -l`), `du -h` reports 8.0M, 7.97 MB (bytes / 1048576).
- Budget (docs/spec.md §5): `< 10 MB` — **PASS**, headroom ~2.03 MB under the cap.
- The six-locale catalog is embedded in the binary: `locales/app.yml` is 23780 bytes
  on disk (`_version: 2`, 76 keys × 6 locales), embedded by `rust_i18n::i18n!("locales",
  ...)` at macro expansion plus the tracked `include_str!` edge (`src/main.rs:72-78`)
  — ~0.28% of the shipped binary; rust-i18n's generated static locale tables, not the
  YAML text itself, carry the i18n size cost.
- Gates around this measurement: `./cargo.sh test --no-run --target x86_64-pc-windows-gnu`
  exit 0, `./cargo.sh build --release --target x86_64-pc-windows-gnu` exit 0 (artifact
  rebuilt fresh at 22:29 local before measuring).

## R7 — CJK/Hangul tofu in the localized UI (follow-up, 2026-11-06)

Symptom (user screenshot on Windows): the language dropdown and every localized label
rendered as tofu boxes for `zh-CN`, `ja`, `ko`; `en`/`ru`/`es` were fine. Root cause is
font coverage, not encoding — the catalog, `t!` and egui text are UTF-8 end to end.
Measured against the bundled `epaint_default_fonts-0.37` cmaps: Ubuntu-Light carries
Latin + Cyrillic (96 Cyrillic glyphs with real outlines), so `ru`/`es` miss 0 chars,
while `zh-CN` misses 74 keys, `ja` 73 and `ko` 73. The binary ships without
`eframe`'s `system_fonts` feature (size budget, spec §5), so no CJK/Hangul face exists.

Rejected options:
- `egui-cjk-font` 0.2.0 — depends on `egui ^0.34` (pulls a duplicate `egui 0.34.3` next
  to our 0.37 → `merge_cjk_font(ui.ctx())` type mismatch), needs native fontconfig via
  `font-kit`, keys on OS locale (`sys-locale`) so zh/ja/ko stay tofu on a ru/en Windows
  machine, and inserts its font FIRST in `Proportional` (changes the Latin face).
- Enabling `system_fonts`: measured release 8909312 bytes vs baseline 8352768 (+556544,
  +0.53 MB) — rejected by the user. It also enumerates fonts and adds `fontique`.
- Embedded font subsets: 0.4–1 MB in the binary against the same budget.

Chosen design (user approved "lazy provider" + Windows candidates + Linux dev-host
candidates, zero new crates.io deps): `src/fonts.rs` registers an
`egui::Context::add_font_provider` (`context.rs:865`) at window creation (`gui::run`
creator closure). egui asks providers only after every face of the family missed the
glyph, at most once per `(family, char)` (`epaint-0.37.0/src/text/font_provider.rs:36-52`),
so Latin/Cyrillic never change. Script is derived from codepoint ranges (Hangul, kana,
CJK blocks) because `epaint::text::FontFace` is not exported — no cmap introspection
from public API. Per script the provider reads the first existing candidate file
(`malgun.ttf`, `yugothic.ttf`, `msyh.ttc` index 0 for `.ttc` collections —
`read-fonts-0.37.0/src/lib.rs:334-346` resolves a collection face by index), caches one
`FontData` per script (misses cached too), and inserts the font as a `Lowest` fallback
into the requesting family. Missing files fall through; if nothing reads, the provider
returns `None` and the glyph stays tofu, exactly as before — no panic.

Verification: 18 new unit tests in `src/fonts.tests.rs` (410 unit tests green,
`SKIP_E2E=1 ./scripts/check.sh` exit 0; `./cargo.sh test --no-run
--target x86_64-pc-windows-gnu` exit 0; `./scripts/check_full.sh` exit 0). On this Linux
dev host the last candidate `/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc` loads and
covers zh-CN/ja/ko per a fontTools cmap probe (0 missing chars for all three locales on
face 0). Windows candidate paths cannot be verified here (no Windows host); the rig is
the acceptance target.

Size re-measure after the change: `target/x86_64-pc-windows-gnu/release/virshot.exe` =
8366592 bytes (8.00 MB), +13824 bytes over the 8352768 baseline — code and log strings
only, no embedded font. Budget `< 10 MB`: PASS, headroom ~1.97 MB. Runtime heap cost is
per-script and only while CJK/Hangul is on screen (order of magnitude: `malgun.ttf`
~5.6 MB, `yugothic.ttf` ~14 MB, `msyh.ttc` ~20 MB); 0 bytes while `en`/`ru` — the
dropdown renders all six endonyms at once, so opening it loads all three scripts.
