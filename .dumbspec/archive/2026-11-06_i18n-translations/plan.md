# 2026-11-06_i18n-translations: implementation plan (UI localization + language dropdown)

Spec — [spec.md](./spec.md); research — [research.md](./research.md).

**Format:** each stage = TDD (red test → green implementation → refactor) + a commit at the end.
This file is a **living progress journal** — update statuses as work proceeds.

Legend: `[ ]` not started · `[~]` in progress · `[x]` done.

---

## Stage 0 — Research + spec + plan

- [x] Research R1 — rust-i18n crate facts (version 4.2.4, `i18n!`, `_version: 2`
      one-file YAML, `set_locale`/`locale()`, missing-key behavior, per-crate catalog,
      dependency chain) → recorded in research.md.
- [x] Research R2 — inventory of user-visible strings (~95) with render sites,
      interpolation, suggested dotted keys, notation families to leave untranslated.
- [x] Research R3 — config model/load/save, startup order (`run_resident`), draft/apply
      flow and mailbox propagation, existing English-pinning tests, spec touchpoints,
      binary-size budget.
- [x] spec.md v1 created (Created + Changes), then user closed all open questions:
      lenient `language` (unknown → `en`), CLI stays English, tray localized with menu
      rebuild, dropdown is the first row of General, `rust-i18n 4.2.4` approved.
- [x] plan.md written (this file).

**Criterion:** `draft.md`, `research.md`, `spec.md`, `plan.md` exist in
`current/2026-11-06_i18n-translations/`; spec "Open questions" = none; scope frozen.
**Commit:** `docs(dumbspec): add i18n UI localization spec and plan`

---

## Stage 1 — Dependency, catalog, `en` key migration

- [x] Add approved dependency `rust-i18n = "4.2.4"` (no features) to root
      `Cargo.toml`; refresh lock via `./cargo.sh check` (done: one manifest line at the
      end of `[dependencies]`, 32 lock packages, `./cargo.sh check -p virshot` green,
      48.99s; features `load-path`/`log-miss-tr` confirmed disabled).
- [x] Red first: `src/i18n.tests.rs` — `locale_default_is_english`,
      `catalog_en_entries_resolve_to_original_english_text`,
      `missing_key_falls_back_to_en_via_fallback` (+ `catalog_is_one_file_with_version_two`)
      (done: 167 lines attached at the end of `src/main.rs`, 34 pinned key→English-string
      assertions, `LOCALE_LOCK` serializes locale-touching tests; `./scripts/check.sh`
      exit 101 — all 34 errors are `E0433: cannot find _rust_i18n_t in the crate root`).
- [x] Wire init in crate root `src/main.rs`: qualified `rust_i18n::i18n!("locales",
      fallback = "en")` at crate root after the inner attributes (`:66-70`), no
      `#[macro_use]` (repo forbids glob-style imports), `minify_key` off. Compile now
      succeeds with an empty catalog; 3 i18n tests fail as expected (key lookups return
      the dotted key, `locales/app.yml` missing) — 361 other tests pass.
- [x] Create `locales/app.yml` with `_version: 2` and **en** entries for every key from
      the inventory (done: 178 lines, 72 keys, en-only — pins matched byte-for-byte plus
      extra keys for Browse, placeholder token descriptions, hotkey/gamepad capture hints
      and tooltips, overlay hints/` ms` suffix, toast error texts, template error texts;
      values quoted where they contain `": "`, leading space, or non-ASCII punctuation;
      notation/CLI/log-only strings excluded). `catalog_is_one_file_with_version_two`
      relaxed to `_version: 2` + `en:` (ru is Stage 3, other locales Stage 4). Gate
      `./scripts/check.sh` exit 0: 364 passed / 1 ignored, e2e 9 passed, clippy clean.
- [x] Replace literals with `t!`: `src/gui.rs` (43 sites — tab labels, General rows,
      Browse, placeholder header + token descriptions via `PLACEHOLDER_DESC_KEYS`,
      hotkey/gamepad capture labels/tooltips, duplicate tooltips with placeholders,
      overlay rows incl. `" ms"`, bottom bar, window title; text consts → functions),
      `src/tray.rs` (4 sites), `src/capture.rs` `saved_text` → `toast.saved`
      (done; the interrupted agent left the tree compiling, the finishing agent closed
      the gap: `src/hotkeys.rs:802-815` `failure_reason` → `hotkey.already_registered`
      / `hotkey.registration_failed`; audit grep of egui/tray/window text APIs returns
      zero raw literals; gate `./scripts/check.sh` exit 0, 364 unit + 9 e2e, clippy clean).
- [x] Chip text for capture errors through catalog keys (done): `Error::Monitor` replaced
      by typed variants `MonitorCaptureFailed` / `WindowTitleFailed` / `WindowCaptureFailed`
      / `WindowListFailed` / `MonitorListFailed` / `NoMonitorsFound` with byte-identical
      `#[error]` Display strings (logs/CLI stay English), `chip_text()` maps every variant
      to `toast.*` keys, new catalog key `toast.window_capture_failed` added (76 `en:` lines),
      tests `chip_text_matches_display_text_under_english_locale` +
      `error_display_stays_english_under_russian_locale` (LOCALE_LOCK pattern) added,
      `capture_monitor_variant_*` renamed to the new variants with the same message
      assertions. Gate `./scripts/check.sh` exit 0 — 366 unit + 9 e2e, clippy clean.
- [x] Localize hotkey registration-failure tooltip templates (done inside the literal
      replacement step): `src/hotkeys.rs` `failure_reason` now renders
      `hotkey.already_registered` (`%{combo}`) and `hotkey.registration_failed`
      (`%{reason}`); backend-unavailable reasons stay English (log-only path).
- [x] Rewrite English-pinning tests (done): `src/gui.tests.rs` gained `LOCALE_LOCK` +
      `locale_guard()` + `assert_en_catalog(key, rendered, contract)` (rendered value goes
      through the catalog key AND the catalog `en` value equals the shipped English
      byte-for-byte); rewritten: `tab_labels_are_english`, `row_labels_are_english_per_spec`,
      `bind_hint_mentions_escape_and_backspace` (`.is_ascii()` proxy dropped — Cyrillic lands
      in Stage 3), duplicate/unbound/pad tooltips, `overlay_test_message_matches_real_save_success_wording`,
      `overlay_test_disabled_hint_names_the_overlay_checkbox`, pad-field label and keyboard/pad
      warning tests. `src/tray.tests.rs::menu_labels_match_spec_section_4_5` now pins the three
      `tray.*` keys. Notation assertions untouched (key names, MOD_NAMES, `+`, pad notation,
      format names, `!`/`i` glyphs). Added
      [`every_catalog_key_resolves_under_english`](../src/i18n.tests.rs) — plain line walk of
      `locales/app.yml` finds exactly 76 keys, each resolves under `en` (non-empty, not the
      dotted key text).
- [x] `./scripts/check.sh` green (exit 0 — 367 unit + 9 e2e passed, 1 ignored, clippy `-D warnings`
      clean, including `--all-targets`).

**Criterion:** every user-visible string comes from the catalog with `fallback = "en"`;
under the default locale the rendered English text is byte-identical to the pre-i18n
strings (asserted by tests); notation strings untranslated; unit tests + clippy pass.
**Commit:** `feat(i18n): rust-i18n catalog with en keys replacing UI literals`

---

## Stage 2 — `general.language`, startup locale, dropdown, live switching

- [x] Red first (config): `general_default_language_is_english`,
      `load_partial_toml_fills_language_default`,
      `load_unknown_language_normalizes_to_english_but_keeps_other_settings`,
      `language_code_maps_all_known_locales`, `save_then_load_round_trip_preserves_language`
      (done: red = 15 compile errors, `E0609 no field language`; green after implementation).
- [x] Red first (gui/tray): `language_options_are_the_six_supported_locales_with_native_names`,
      `language_dropdown_default_shows_english`,
      `locale_code_of_draft_matches_config_normalization`,
      `tray_menu_labels_render_through_catalog_keys`,
      `tray_menu_rebuild_uses_current_locale_labels` (done: red = 13 compile errors
      `E0425` for `LANGUAGE_OPTIONS`/`native_name`/`locale_code_of`/`menu_labels`/`build_menu`;
      green after implementation. The rebuild test pins item ids/labels/order by shape — muda
      auto-numbers predefined separator ids, so the separator slot cannot be pinned by id).
- [x] Dropdown as **first row of General** (`src/gui.rs:1759-1769`, before the Save path row):
      `egui::ComboBox::from_label(t!("general.language_label"))` + `native_name()` endonyms from
      `LANGUAGE_OPTIONS: [(&str, &str); 6]` (:292, `en` first; endonyms are constants, never
      localized), selection writes the canonical code into `draft.general.language`, so the
      existing dirty/enable logic covers it. `locale_code_of()` (:316) wraps `config::language_code`.
- [x] `SettingsApp::apply()` (:1448-1466): `config::save` → `rust_i18n::set_locale(locale_code_of(...))`
      → `tray.refresh_menu()` (labels rebuilt for the current locale via the shared `build_menu()`;
      `tray-icon 0.26.1::set_menu(Option<Box<dyn ContextMenu>>)` is infallible per the vendored
      source, so the only failure mode is `build_menu`, logged with `tracing::warn!` and never fatal).
- [x] `./scripts/check.sh` green (377 unit + 9 e2e, clippy `-D warnings` clean).
- [x] Add `General.language: String` (first field, container default covers old configs) +
      `pub(crate) fn language_code(raw: &str) -> &'static str` in `src/config.rs`
      (done: field at :403, default `"en"` in `general_with_home` :429, mapping at :112-129 —
      EXACT match on `en/ru/zh-CN/es/ja/ko`, anything else → `en`; normalization applied in
      `load_or_default` Ok-branch :140-144 so only `language` is repaired, other settings kept).
- [x] Startup locale: after `config::load_or_create` in `run_resident`
      (`src/main.rs:333-341`) `rust_i18n::set_locale(config::language_code(...))` before the
      instance guard, tray and `gui::run`; headless `run_shot` stays English (spec §2).
- [ ] Dropdown as **first row of General** (`general_tab`, before the Save path row,
      `src/gui.rs:1627-1639`): `egui::ComboBox::from_label(t!("general.language_label"))`
      with native locale names (`English`, `Русский`, `简体中文`, `Español`, `日本語`,
      `한국어`) bound to `draft.general.language`.
- [ ] `SettingsApp::apply()`: after `config::save`, call `set_locale(language_code(...))`
      and rebuild tray menu labels (labels built in `TrayController::new`,
      `src/tray.rs:125-146` → add a rebuild/update path).
- [ ] Make the red gui/tray locale tests green; refactor duplicated locale-code mapping.
- [ ] `./scripts/check.sh` green.

**Criterion:** the dropdown is the first row of General and defaults to `English`;
changing the language and pressing Apply/OK switches GUI + tray text immediately
without a restart; `language` is persisted in `config.toml`; an unknown value loads as
`en` and leaves other settings intact.
**Commit:** `feat(config,gui): language setting in General with live locale switching`

---

## Stage 3 — Russian translations (priority)

- [x] Red first: `every_catalog_key_has_ru_translation` (76 keys, `ru:` line present and
      different from `en:`), `placeholder_names_survive_in_ru` (same `%{...}` set per locale),
      `ru_tab_labels_render_in_russian`, `ru_interpolated_toast_substitutes_placeholders`,
      `tab_labels_follow_russian_locale`, `placeholder_reference_lines_follow_russian_locale`,
      `tray_menu_labels_follow_russian_locale`, `chip_text_renders_in_russian_under_ru_locale`
      (done: red = 8 failures, e.g. chip `left: "no active window"` vs `right: "нет активного окна"`).
- [x] Fill `ru` for **every** key in `locales/app.yml` (done: +76 `ru:` lines right after each
      `en:` line, add-only diff — en values and key order byte-unchanged; placeholders byte-identical;
      notation kept: `ru: "OK"`, `ru: "virshot"`, `ru: " мс"` with the leading space, `%Y`/`%t`/`%iso`,
      `"fullscreen"`, `png` inside Russian descriptions).
- [x] Green; placeholders verified to substitute (`Сохранено: shot.png`, `Дублирование Окно: Ctrl+P`).
- [x] `./scripts/check.sh` green (385 unit + 9 e2e, clippy `-D warnings` clean).
- [x] Build-edge fix found during Stage 3: rust-i18n 4.2.4 reads the catalog at macro expansion
      and its build-script `rerun-if-changed` points at the dependency crate's own directory, so cargo
      does NOT rebuild on `locales/app.yml` edits (the agent had to `touch src/main.rs`). Recorded in
      research.md (R5); fixed with a tracked compile edge — `const I18N_CATALOG_SOURCE: &str =
      include_str!("../locales/app.yml")` at the crate root (`src/main.rs:72-78`) plus a startup-only
      `tracing::debug!` consumer (`:348-351`) so it is a real use, and
      `catalog_source_is_embedded_as_a_compile_dependency` pinning embedded == on-disk bytes.
      Dep-edge verified empirically: a catalog edit re-checks `virshot`, a no-op run does not.

**Criterion:** with locale `ru` every UI string renders in Russian — no key falls back to
English or to its own key text; placeholders still substitute correctly.
**Commit:** `feat(i18n): Russian translations for all UI keys`

---

## Stage 4 — zh-CN, es, ja, ko translations

- [x] Red first: extend the catalog completeness test to require **all six** locales
      (`en`, `ru`, `zh-CN`, `es`, `ja`, `ko`) for every key; add a placeholder-integrity
      test (every locale keeps the same `%{...}` placeholder set). (done: `every_catalog_key_has_ru_translation`
      → `every_catalog_key_has_all_six_locale_translations`, `placeholder_names_survive_in_ru` →
      `placeholder_names_survive_in_every_locale`; red = 7 failures, first: chip
      `left: "no active window"` vs `right: "没有活动窗口"`; the fallback probe now uses `de`,
      outside the catalog, since `es` has entries.)
- [x] Add `zh-CN`, `es`, `ja`, `ko` translations for all keys (done: 304 add-only lines,
      76 keys × 4 locales, appended after each `ru:` line; en/ru values and key order byte-unchanged.
      Two YAML quirks fixed while greening: `%`-leading values in `toast.too_many_collisions`
      (zh-CN/ja/ko) needed double quotes — `%` is a reserved YAML indicator — and one missing
      `\"` escape in the es `paths.template_parent_segment`; the rust-i18n proc-macro parses the
      file at compile time, so both surfaced as parse panics.)
- [x] Green; per-locale assertions added: `all_six_locales_render_without_fallback_to_key_text`,
      `zh_cn_es_ja_ko_tab_labels_render_locally`, `interpolated_keys_substitute_placeholders_in_every_locale`,
      `placeholder_reference_lines_render_in_each_locale`, `tray_menu_labels_render_in_each_locale`,
      `chip_text_renders_in_each_locale_and_display_stays_english` (chip follows locale, `Display` stays English).
- [x] `./scripts/check.sh` green (392 unit + 9 e2e, clippy `-D warnings` clean).

**Criterion:** completeness test passes for six locales; placeholder integrity test
passes; switching to each locale renders localized text (verified per locale by tests).
**Commit:** `feat(i18n): zh-CN, es, ja, ko translations`

---

## Stage 5 — Project docs, full gates, release-size check

- [x] Update `docs/spec.md`: §2 i18n bullet (`:101-102`), §4.3 toast wording (`:321`),
      §4.4 UI language (`:360`), §4.5 tray menu labels (`:488`), §7 `[general]` schema
      with `language` (`:547-577`) + corrupt-config note (`:541`), §10 testing list
      (`:708 ff`). (done: +74/-11 — §2 bullet now states UI IS localized with the six locales
      and the English-only CLI/logs exception; §4.3 chip wording comes from `toast.*` while
      `Error: Display` stays English; §4.4 describes the dropdown as the first General row and
      the Apply order save → set_locale → tray rebuild; §4.5 localized menu labels rebuilt on
      Apply with the `virshot` tooltip unchanged; §7 adds `language = "en"` as the first
      `[general]` key plus the lenient normalization note; §10 lists the i18n test coverage and
      the per-thread locale serialization. A v9 changelog entry and a §3 stack bullet for the
      approved `rust-i18n` dep were added in-style.)
- [x] `./scripts/check_full.sh` green (check + test + clippy -D warnings + fmt — 392 unit + 9 e2e).
- [x] Windows test compile: `./cargo.sh test --no-run --target x86_64-pc-windows-gnu` exit 0
      (three test executables built).
- [x] Release build `./cargo.sh build --release --target x86_64-pc-windows-gnu` exit 0;
      `virshot.exe` = 8 352 768 bytes = **7.97 MB** — inside the `<10 MB` budget (headroom
      ~2.03 MB); catalog embed cost is negligible (23 780 bytes of YAML, ~0.28%). Recorded in
      research.md (R6).
- [x] `./cargo.sh test --workspace` green at the stage boundary (gamepad 13, icons 2+1+7,
      overlay 50, toast 43, virshot 392 unit + 9 e2e).

**Criterion:** project spec matches implemented behavior; formatting-aware gate green;
Windows-gnu test binaries compile; release binary within the size budget (size recorded
in research.md).
**Commit:** `docs(spec): localized UI strings and the language setting in General` — done
(`5e02c39`).

---

## Done (2026-11-06)

All five stages landed on branch `2026-11-06_i18n-translations`, one commit per stage:
`9d5c4e4` (spec/plan) → `40d12b9` (catalog + `t!` migration) → `f641679` (config language,
dropdown, live switch, tray rebuild) → `5632240` (ru, + catalog rebuild compile edge) →
`e0789a6` (zh-CN/es/ja/ko) → `5e02c39` (docs + final gates).

Final state: 76 keys × 6 locales in `locales/app.yml`; 392 unit + 9 e2e tests green;
`./scripts/check_full.sh`, `./cargo.sh test --workspace`, windows-gnu test compile and
release build all exit 0; release binary 7.97 MB (< 10 MB budget). Notation, CLI text and
`Error: Display` remain English (spec §2/§8); UI text, tray labels and toast chips localize
without a restart (spec §4.4/§4.5).

## Follow-up (2026-11-06): localized CJK/Hangul text renders as tofu on Windows

Found by the user after Stage 5 shipped: the language dropdown and localized labels for
zh-CN/ja/ko showed tofu boxes (`en`/`ru`/`es` fine). Coverage measurement against the
bundled faces (`research.md` R7) — Ubuntu-Light covers Latin + Cyrillic, CJK/kana/Hangul
have no glyph, and `eframe`'s `system_fonts` feature is off for the size budget.
`egui-cjk-font` and enabling `system_fonts` (+0.53 MB) were rejected; the approved fix is
`src/fonts.rs`, a lazy `egui::Context::add_font_provider` with zero new crates.io deps:
script from codepoint ranges, first readable system font per script, cached, appended as a
`Lowest` fallback to the requesting family; missing files degrade to tofu, never panic.
Registered in `gui::run`'s window-creation closure so every viewport (settings, toast,
overlay) localizes. 18 unit tests in `src/fonts.tests.rs`; 410 unit green, Windows test
compile green, `check_full.sh` green, release 8366592 bytes (+13824, budget PASS).
