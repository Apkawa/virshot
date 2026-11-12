# 2026-11-12_gui-refactor-update-module: декомпозиция gui и перенос модулей автообновления

**Created:** 2026-11-12T12:56:09+03:00

## Scope

**In scope:**
1. `src/gui.rs` (фактически 3912 строк) разносится на функциональные модули в `src/gui/`:
   каждый файл отвечает за одну функцию (по имени файла).
2. Все модули автообновления переезжают в `src/update/` вместе с тестами:
   - `src/update.rs` → `src/update/mod.rs`
   - `src/update_install.rs` → `src/update/install.rs`
   - `src/update_notes.rs` → `src/update/notes.rs`
   - тесты: `update.tests.rs` → `update/update.tests.rs`, `update_install.tests.rs` →
     `update/install.tests.rs`, `update_notes.tests.rs` → `update/notes.tests.rs`
3. Обновление путей вызова (`crate::update_install::` → `crate::update::install::`,
   `crate::update_notes::` → `crate::update::notes::`) в `main.rs` и `gui`.
4. Тесты `gui.tests.rs` (2850 строк) разбираются по submodule-файлам по правилу
   «один тестовый файл на модуль».

**Out of scope:**
- Изменение поведения приложения — это чистый перенос кода, функциональность не меняется.
- Новые зависимости (не нужны: ureq, self-replace, egui_commonmark уже зафиксированы).
- Перевод gui на lib-таргет / integration-тесты в `tests/unit/` — структура остаётся bin-only.

## Подход

### Перенос автообновления (`src/update/`)

- `update.rs` становится корнем директории: `src/update/mod.rs` несёт контракт release.yaml
  (manifest, check, deadline) и объявляет `pub(crate) mod install; pub(crate) mod notes;`.
  Это оставляет `crate::update::…` пути вызова (`spawn_check`, `next_check_deadline`,
  `current_build`, `CheckOutcome`, `UpdateInfo`) **без изменений** в `main.rs` и `gui`.
- Ре-экспортов в `mod.rs` для install/notes не делаем: call-sites явно пишут
  `update::install::…` / `update::notes::…` (несколько строк в `main.rs` и `gui`).
- Тесты переезжают рядом с модулями и остаются `#[path = "...tests.rs"] mod tests;`
  с `use super::*` (в `install.rs`/`notes.rs` вместо `crate::update::UpdateInfo` → `super::UpdateInfo`).

### Разнесение `src/gui.rs`

Функциональные модули (по уже видимым блокам кода, см. research.md):

| Файл | Ответственность | Исходные строки gui.rs |
| --- | --- | --- |
| `gui/mod.rs` | обзорный doc модуля, объявления submodules, `pub(crate) use window::run` | 1–~143 (doc) |
| `gui/app.rs` | `SettingsApp` (состояние, `new`, `apply`, `binding`/`binding_mut`, `request_window_exit`), `impl eframe::App` (tray poll, About viewport, update scheduling/drain, close decision, ui-стрип), `TRAY_POLL_TICK`, `Tab`, doc-заметки про tray/toast/close (§4.5) | 197, 1839–2005, 2007–2223 (кроме update-полей-хелперов), 2276, 3073–3691 |
| `gui/general.rs` | таб General: `general_tab` + `browse_save_path` + хелперы `browse_start_dir`, placeholder-аккордеон, `format_label`, язык/`LANGUAGE_OPTIONS`/`locale_code_of`, контролы update-interval/channel/auto-install + `update_check_selected_text` | 206–378, 2484–2722 |
| `gui/hotkeys.rs` | таб Hotkeys: `hotkeys_tab`, `HOTKEY_ROWS`/`row_label`, статусные хелперы (`status_icon_for`, `registration_failure`, `has_failed_registration`, `paint_warning_slot`) и capture-wiring-методы (`set_keyboard_capture`, `start_pad_capture`, `cancel_*`, `drain_pad_capture`, `apply_capture_transition`, `handle_capture_press`, `commit_probe_press`, `handle_pad_capture_press`) | 265–268, 2087–2158, 2224–2483, 2760–2905 |
| `gui/binding.rs` | чистая логика биндинга: `SUPPORTED_KEY_NAMES`, `MOD_NAMES`, key-capture (`supported_key_name`, `binding_label`, `mods_from_press`, `binding_from_press`, `probe_binding_from_press`, `clear_*`, `bind_button_label`), label/tooltip-хелперы, duplicate/save-гейт, registration retry, capture state-machine | 412–1090 |
| `gui/overlay.rs` | таб Overlay: `overlay_tab` + `publish_overlay_test_chip` + overlay-test чип (`OVERLAY_TEST_FILENAME`, `overlay_test_*`, `test_overlay_toast`), `position_label` | 332–378, 2906–2977 |
| `gui/update.rs` | update UI: статусы, диалог (`UpdateDialogWindow`/`View`/`paint_update_dialog`), чипы discovery/failure, forced-check/schedule-решения, install-хелперы, методы `open_update_dialog`/`dismiss_update_dialog`/`start_install`/`publish_update_chip`, doc-заметки §4.7 | 1274–1826, 2996–3072 |
| `gui/icons.rs` | статус-иконки и текстуры (`IconKind`, `StatusIcon`, `icon_for_status`, glyph/color, `warning_icon_texture`, `about_icon_texture`, `WARNING_COLOR`/`INFO_COLOR`, `BIND_BUTTON_MIN_WIDTH`) | 586, 625–631, 1092–1206 |
| `gui/chrome.rs` | нижняя панель и close-семантика: `is_dirty`, `BottomButtons`, `bottom_buttons_enabled`, `BOTTOM_BAR_PANEL_ID`, `cancel_draft`, `CloseAction`, `close_decision` | 1208–1266 |
| `gui/window.rs` | запуск окна: `run`, `host_monitor_rects`, `app_window_icon`, `INITIAL_WINDOW_WIDTH/HEIGHT`, `Error` | 190–191, 1831–1834, 3710–3908 |

Ключевые решения:
- **UI-методы `SettingsApp` разъезжаются по таб-файлам** (выбор пользователя, вариант B):
  `impl SettingsApp` живёт в нескольких файлах модуля `gui` (разные impl-блоки — легальный Rust);
  поля структуры становятся `pub(super)`, методы, используемые другими файлами — `pub(super)`.
  Видимость остаётся в границах `gui/` — crate-wide `pub` нигде не появляется.
- `app.rs` несёт состояние, `apply`, логику event-loop; таб-файлы несут paint-табов
  и свою capture/logic-обвязку.
- Module-doc из gui.rs (1–143) разъезжается по своим местам: обзор в `mod.rs`,
  заметки про tray/toast/close в `app.rs`, заметки §4.7 (update scheduling) в `update.rs`.
- `update_chip_geometry` остаётся локальной дубликацией monitor→corner mapping (KISS, как сейчас).
- Тесты разбиваются `gui.tests.rs` на sibling-файлы `<module>.tests.rs` по areas из research.md:
  binding ≈74–520, icons 523–575/1487–1520, chrome 577–1069, general-labels/placeholders/language
  1070–1133/1544–1685/1863–2076, pad/capture-transitions 1135–1175/1208–1485, registration retry
  1699–1861, update UI 2079–2850, app/logic-тесты — оставшиеся.
  Каждый тестовый файл начинается с `use super::*` + нужные imports из sibling-модулей.
- Внешние ссылки тестов обновляются: `crate::gui::binding_label` (hotkeys.tests.rs) →
  `crate::gui::binding::binding_label`; `gui::OVERLAY_TEST_FILENAME` / `gui::overlay_test_message`
  (i18n.tests.rs) → `gui::general::…`.

## Constraints

- Rust edition 2024, bin-only root package; форматирование только в коммите (`cargo fmt`),
  итерационный гейт — `./scripts/check.sh` (check + test + clippy `-D warnings`), в песочнице
  через `./cargo.sh`.
- `#![forbid(unsafe_code)]`, `#![deny]` не трогаем; никаких `unsafe`.
- Тесты размещаются по видимости: приватные internals → sibling `<module>.tests.rs`
  (конвенция уже используется в 17 местах репозитория).
- `update.rs` зависит от `crate::about::version_with_dev` и `crate::build::*` — из
  `src/update/mod.rs` эти пути остаются reachable (тот же crate).
- restart-контракт (`restart_requested` + `ViewportCommand::Close`, `self_replace` seam)
  не меняется.
- Факт из исследования: `gui.rs` — 3912 строк, не ~3000; это уточнение к исходному запросу.

## Open questions

- none (структурная разбивка выбрана; подтверждается в плане перед исполнением)

## Changes

- 2026-11-12T12:56:09+03:00 -- v1, initial spec (draft + research)
- 2026-11-12T13:37:06+03:00 -- пользователь выбрал вариант B: UI-методы `SettingsApp`
  разъезжаются по таб-файлам (`general.rs`, `hotkeys.rs`, `overlay.rs`), поля структуры
  становятся `pub(super)` вместо «все методы остаются в app.rs»
