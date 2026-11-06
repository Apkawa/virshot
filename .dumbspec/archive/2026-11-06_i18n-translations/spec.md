# 2026-11-06_i18n-translations: локализация UI (i18n + язык в General)

**Created:** 2026-11-06T18:17:08+03:00

## Scope

**In scope**
- Новый crates.io-депенденс `rust-i18n` (4.2.4) в корне `virshot`, инициализация
  `i18n!("locales", fallback = "en")`.
- **Один файл переводов** `locales/app.yml` с `_version: 2` — все локали в одном
  файле (паттерн rust-i18n «All Localized Texts in One File»).
- Перенос существующих видимых строк (~95) в ключи; переводы: `en`, `ru`, `zh-CN`,
  `es`, `ja`, `ko` (`ru` — в первую очередь, остальные — автоматические переводы с
  последующей правкой пользователя).
- Dropdown выбора языка в секции **General** окна настроек; значение хранится в
  конфиге (`general.language`), применяется **в рантайме** через
  `rust_i18n::set_locale()` без перезапуска.
- Локализация: окно настроек (вкладки General/Hotkeys/Overlay, нижняя бар-кнопки,
  тултипы), tray-меню, toast-чипы (тексты успеха/ошибок), tooltip'ы причин
  регистрации хоткеев.
- Обновление `docs/spec.md` (§2, §4.3, §4.4, §4.5, §7, §10) под новый behavior.
- Перестройка существующих тестов, которые pin'ят английский текст, + новые тесты.

**Out of scope**
- CLI/`--help`/stderr/stdout текст — остаётся **английским** (текущая запись в
  `docs/spec.md` §2 про ключи конфига и CLI). Подтверждение ниже.
- Нотации, которые не являются «текстом для чтения»: имена клавиш
  (`src/gui.rs:333-442`), `MOD_NAMES` (`:446`), разделитель `+`, pad-нотации
  (`unknown:N`, `Guide`, `Share`), токены шаблонов (`%Y` …), `png/jpg/webp`,
  глик-символы `!`/`i`, `test.png`.
- Переводы `crates/toast`/`crates/overlay` как отдельных crates: тексты чипов
  формируются в корневом crate, поэтому их ключи живут в корневом `locales/app.yml`.
  Единственная строка `crates/overlay` (`"virshot overlay"` — невидимый заголовок
  окна, `crates/overlay/src/gl_boot.rs:198`) не локализуется.
- Автоопределение языка из системы, locales кроме шести, локализация README/доков.

## Approach

- **Депенденс:** `rust-i18n = "4.2.4"`, без features (не нужны `load-path`
  и `log-miss-tr`). Чистый Rust, без native-библиотек; тексты встраиваются в бинарь
  на этапе компиляции (чтение `locales/` с `CARGO_MANIFEST_DIR`, runtime-чтения нет).
  Добавление требует явного подтверждения пользователя (правило проекта).
- **Инициализация:** `#[macro_use] extern crate rust_i18n;` + `i18n!("locales",
  fallback = "en")` в crate root (`src/main.rs`); `t!` доступен во всех модулях
  корневого crate. `fallback = "en"` обязателен: rust-i18n при отсутствии ключа
  возвращает сам текст ключа и **не** фолбэкает на `en` автоматически.
- **Файл переводов:** `locales/app.yml`, одна секция на ключ, внутри — коды локалей:

  ```yml
  _version: 2
  general.tab_general:
    en: General
    ru: Общие
    zh-CN: 常规
    es: General
    ja: 一般
    ko: 일반
  toast.saved:
    en: Saved: %{file_name}
    ru: Сохранено: %{file_name}
  ```

  Ключи — точечные (dot-separated), сгруппированные по области: `general.*`, `hotkey.*`,
  `gamepad.*`, `overlay.*`, `toast.*`, `paths.*`, `tray.*`.
- **Ключи и подстановки:** динамические части → `%{...}`-плейсхолдеры
  (`t!("toast.save_failed", path = p, reason = r)`). Порядок слов в переводе
  задаётся текстом ключа, не конкатенацией в коде.
- **Конфиг:** новое поле `general.language: String`, дефолт `"en"`
  (container-level `#[serde(default)]` уже покрывает отсутствующие ключи и старые
  конфиги).
  Политика битого значения — **мягкая**: неизвестное/битое значение нормализуется в
  `"en"` (helper в `src/config.rs`, например `language_code(raw) -> &'static str` из
  набора `en/ru/zh-CN/es/ja/ko`), поэтому `load_or_default` не сбрасывает остальные
  настройки (хоткеи, пути, overlay) в дефолты. Unknown-ключи serde и так игнорирует
  (нет `deny_unknown_fields`), старые конфиги продолжают работать.
- **Применение языка:**
  - стартовое: сразу после `config::load_or_create` в `run_resident`
    (`src/main.rs:326`) и **до** построения tray (`:392`) и `gui::run` (`:404`);
  - живое переключение: в `SettingsApp::apply()` (`src/gui.rs:1327-1361`) после
    сохранения конфига — `rust_i18n::set_locale(code)` + перестроение tray-меню
    (labels создаются в `TrayController::new`, `src/tray.rs:125-146`) и
    перерисовка GUI (egui строит текст каждый кадр).
  - `language` внутри `Settings` автоматически доходит до listener/pad-pump/capture
    через shared mailbox (`Arc<Mutex<Settings>>`, `src/main.rs:356`,
    `src/hotkeys.rs:486`).
- **Dropdown:** `egui::ComboBox::from_label(...)` **первой строкой секции General**,
  перед «Save path» (`general_tab`, `src/gui.rs:1627-1639`). Имена локалей в dropdown —
  собственные (native) названия и **не** переводятся: `English`, `Русский`,
  `简体中文`, `Español`, `日本語`, `한국어`; internally — коды
  `en/ru/zh-CN/es/ja/ko` (они же — ключи локалей в каталоге, регистр чувствителен).
- **Тесты (TDD):**
  - новый sibling-модуль `src/i18n.tests.rs` (или additions в существующих) —
    completeness-тест каталога: каждый ключ содержит все 6 локалей (сканирование
    текста файла, без нового dev-dep), `locale_default_is_english`,
    переключение `set_locale` меняет текст ключа;
  - config: `language_default_is_english`, частичный TOML заполняет дефолт,
    round-trip, политика битого значения;
  - gui: существующие `tab_labels_are_english` / `row_labels_are_english_per_spec`
    / `bind_hint_mentions_escape_and_backspace` / tray `menu_labels_match_spec_section_4_5`
    переписываются как assertions дефолтной локали `en` + новый тест «ярлыки следуют
    локали» (например `ru`);
  - `./scripts/check.sh` как итерационный гейт, `./scripts/check_full.sh` на границе
    этапов.
- **Размер бинаря:** бюджет `docs/spec.md` §5 — <10 MB; после добавления депенденса
  проверяется release-сборкой `--target x86_64-pc-windows-gnu`.

## Constraints

- Rust stable 1.99.1, edition 2024; cargo в песочнице только через `./cargo.sh`
  (`CARGO_HOME=./.cache/cargo`).
- `#![forbid(unsafe_code)]` в корневом crate (third-party unsafe внутри rust-i18n —
  вне правила проекта).
- **Новый crates.io-депенденс требует явного подтверждения пользователя** (AGENTS.md).
- Workspace members: `[".", crates/overlay, crates/toast, crates/gamepad, crates/icons]`,
  exclude `hidpad-poc`.
- `rust-i18n` facts (research.md R1): init-макро — `i18n!` (нет `i18n_init!`);
  `_version: 2` — обычный YAML/JSON/TOML-файл в `locales/`, локаль — вложенный ключ;
  `set_locale`/`locale()` процесс-широкие, но каталог **на crate** (каждый crate с
  собственным `i18n!`); кейсы локалей чувствительны к регистру; `t!` → `Cow<str>`;
  отсутствующий ключ → текст ключа (не пусто); `[package.metadata.i18n] default-locale`
  не задаёт стартовую локаль → нужен явный `set_locale()`.
- Конфиг пишется полным перезаписыванием (`toml::to_string_pretty`, не атомарно),
  только в `config::save` и GUI `apply()`.
- Бюджет релизного бинаря <10 MB (§5).
- Локали `ru/zh-CN/es/ja/ko` — автопереводы; пользователь правит позже.

## Open questions

none — все закрыты пользователем (2026-11-06):
битое значение `general.language` → мягкая нормализация в `en`; CLI остаётся
английским; tray локализуется с перестроением меню на Apply; dropdown — первой
строкой секции General; депенденс `rust-i18n 4.2.4` подтверждён.

## Changes

- 2026-11-06T18:17:08+03:00 -- v1, initial spec (draft + research R1–R3: crate facts,
  inventory of ~95 strings, config/startup/apply wiring, test pins).
- 2026-11-06T18:28:45+03:00 -- пользователь закрыл open questions: мягкая политика для
  битого `language` (неизвестное → `en`, остальные настройки сохраняются); CLI не
  локализуется (английский); tray локализуется и перестраивается на Apply; dropdown
  выбранной языка — первой строкой General; добавление `rust-i18n 4.2.4` подтверждено.
