# 2026-11-05_icons-crate: иконки для virshot

**Created:** 2026-11-05T12:15:06+03:00

## Scope

- Новый workspace-крейт `crates/icons`: SVG-источники иконок → генерация PNG-растров → встроенные (`include_bytes!`) ассеты в бинарник.
- Пример `crates/icons/examples` — простое eframe-окно, показывающее все иконки крейта.
- Первые две иконки и их внедрение:
  1. **Иконка приложения** (камера) — заменяет `tray::camera_glyph_rgba()`; используется для трея **и** иконки окна (titlebar/taskbar).
  2. **Иконка предупреждения** — жёлтый треугольник с чёрным `!` в центре (`⚠`); используется в `gui.rs` вместо текстового glyph для warn-статуса.
- Механизм расширяем: добавление новой иконки = SVG + перегенерация + одна константа/путь. Никаких спекулятивных абстракций (KISS).
- Out of scope: runtime-растеризация SVG внутри egui (требует `egui_extras`/`resvg` — новые deps, не одобрены); иконки, не перечисленные выше.

## Approach

- **Генерация** — внешняя утилита `rsvg-convert` (librsvg2-bin 2.58.0, уже установлена; librsvg- crate собрать нельзя — `pkg-config --exists librsvg` не проходит). Скрипт `scripts/gen-icons.sh`: обход `crates/icons/assets/svg/*.svg` → `rsvg-convert -f png -w <size> -h <size>` → `crates/icons/assets/png/<name>-<size>.png`. SVG и PNG ассеты — трек в git (в репозитории сейчас нет ни одного PNG/SVG-файла).
- **Формат в бинарнике** — PNG, встроенные через `include_bytes!`. Декод при запуске уже доступным `image` 0.25 (`load_from_memory → into_rgba8`); новых crates.io-зависимостей нет.
- **API крейта**: `bytes(name) -> &'static [u8]` (или `pub const APP_ICON_PNG: &[u8]` + аналогично для warning); конвертация в нужные форматы ефрей-стека:
  - иконка окна: `egui::IconData { rgba, width, height }` (straight alpha) → `ViewportBuilder::with_icon(...)` в `gui::run`;
  - трей: `tray_icon::Icon::from_rgba(rgba, w, h)` в `tray.rs` (замена `camera_glyph_rgba()`);
  - отрисовка в UI (gui.rs, example): `image::load_from_memory` → `egui::ColorImage::from_rgba_unmultiplied` → `ctx.load_texture(...)` один раз при инициализации → `ui.painter().image(...)` — тот же паттерн, что `crates/overlay/src/content.rs`.
- **Размеры**: SVG с квадратным viewBox 128×128; генерируемый PNG 128×128 (HiDPI-качество; для трея/окна winit/eframe сами масштабируют 128-растр до 16/32; для UI-отрисовки in-app — масштабирование текстурой с linear-фильтром). Один размер на иконку — проще (KISS).
- **Конвенции крейта** (по образцу overlay/toast): `name = "icons"`, `version = "0.1.0"`, `edition = "2024"`; deps только из уже locked-набора (`egui = "0.37"`, `image` png+jpeg+webp, `thiserror = "2"`); `[dev-dependencies] eframe` для example; `#![forbid(unsafe_code)]`; один модуль = один файл; inline-тесты в конце модуля (конвенция crates/); example с head-doc `xvfb-run -a ./cargo.sh run --example icons_gallery -p icons`.
- **Внедрение**:
  - `src/tray.rs`: `camera_glyph_rgba()` удаляется; `TrayController::new` получает RGBA из декода встроенного PNG. Тесты `tray.tests.rs` адаптируются (assertы на ассет вместо процедурного рисунка).
  - `src/gui.rs`: `NativeOptions.viewport.with_icon(app_icon IconData)`; warn-статус рисует текстуру warning-иконки вместо ASCII `"!"`.
  - корневой `Cargo.toml`: `icons = { path = "crates/icons" }` + member в `[workspace] members`.

## Constraints

- Новые crates.io-зависимости требуют явного подтверждения пользователя. План не добавляет ни одной: `egui`/`image`/`thiserror`/`eframe(dev)` уже в lock и в одобранном наборе; генератор — внешний CLI `rsvg-convert` (установлен, пользователь подтвердил).
- `egui::IconData`/winit требуют RGBA8 straight alpha, `len % 4 == 0`, `w*h == len/4`; square; X11 работает (`_NET_WM_ICON`), Windows: ICON_SMALL (winit) + ICON_BIG (eframe, capture на создании окна); **Wayland — no-op** (winit 0.30.13): иконка окна на Wayland не ставится — ограничение стека, не баг.
- egui 0.37 не декодирует PNG сам (`Loaders::default()` пуст, loaders не установлены) → декод через `image` до загрузки текстуры; загрузка текстуры — один раз вне UI-цикла.
- Gates: `./scripts/check.sh` (root package; icons компилируется как root-dep), crate-тесты `./cargo.sh test -p icons`, `cargo fmt` покрывает всех members; все cargo-вызовы через `./cargo.sh`.
- `#![forbid(unsafe_code)]` (чистый Rust, как toast).

## Open questions

- none (размеры и путь генерации решены выше; единственная внешняя зависимость — наличие `rsvg-convert` на хосте генерации).

## Changes

- 2026-11-05T12:15:06+03:00 -- v1, initial spec (draft + research). Решения: генератор — CLI `rsvg-convert` (не Rust-библиотека); формат в бинарнике — PNG через `include_bytes!`; окно/трей через `egui::IconData`/`tray_icon::Icon::from_rgba`; один raster-размер 128×128 на иконку.
