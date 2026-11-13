# 2026-11-13_autostart: Добавление в автозагрузку

**Created:** 2026-11-13T00:46:29+03:00

## Scope

- Новая опция конфига `general.autostart` (bool, дефолт `false`), существует только в Windows-сборке (поле и ключ TOML под `#[cfg(windows)]`).
- Вкладка General: чекбокс «Добавить в автозагрузку». При сохранении настроек (Apply/OK, общий путь `SettingsApp::apply()`): если включён — регистрация в автозапуск Windows; если снят — удаление из автозагрузки. Cancel не сохраняет и ничего не меняет в реестре.
- При старте приложения (резидентный режим `run_resident`) состояние опции из конфига синхронизируется с автозагрузкой: добавление/удаление соответственно — это покрывает ручное редактирование конфига.
- Windows-only: на Linux чекбокс отображается как disabled-заглушка (по прецеденту поля геймпада), никакой registry-доступ не выполняется, ключ TOML отсутствует.
- Linux — вне скопа задачи.

## Out of scope

- Linux-реализация автозагрузки (systemd, XDG autostart).
- Автозагрузка при `virshot shot` (headless-режим) — только резидентный запуск.
- Toast/UI-уведомления о результате регистрации (решение: только лог).

## Approach

- Новый root-модуль `src/autostart.rs` (один модуль = один файл): pure-логика (имя значения реестра, формирование payload из пути exe, решение «записать/удалить/пропустить» по сравнению с текущим значением) тестируется на Linux через `#[cfg(any(test, windows))]`; сами registry-операции под `#[cfg(windows)]`, на Linux — dual-заглушка no-op (прецедент `src/gamepad.rs`, двойной `spawn_pump`).
- Реестр: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, имя значения `virshot`, значение — путь исполняемого файла (`std::env::current_exe()`, прецедент `main.rs:278`), кавычки по конвенции Windows Run. Идентичное текущее значение → не пишется; включено→нет записи = запись; снято→запись есть = удаление; снято→записи нет = no-op (идемпотентно).
- **Зависимость для реестра:** корневой crate запрещает `unsafe` (`#![forbid(unsafe_code)]`), ручной FFI-шим невозможен в `src/`. Решение: `winreg` — pure-Rust обёртка WinReg, target-scoped `[target."cfg(windows)".dependencies]`, `unsafe` внутри стороннего crate (вне правил проекта). Требует явного подтверждения пользователя (правило AGENTS.md) — решение вынесено на утверждение перед исполнением.
- Хук сохранения — ровно существующий путь `SettingsApp::apply()` (`src/gui/app.rs:362-422`): после успешного `config::save` вызывается синхронизация автозагрузки по `draft`. Хук старта — `run_resident` после `config::load_or_create` и guard-а экземпляра, в той же форме, что locale-sync (:429) и arm_startup_check (:518). Оба хука исполняются только на Windows (`cfg!(windows)`).
- Ошибки registry — типизированный `thiserror`-enum в модуле, на уровне приложения логируются и не прерывают сохранение/старт (правило: loop логирует и продолжает). Конфиг-фоллбек (битый конфиг → defaults + ERROR) автоматически даёт `autostart=false` → синхронизация на старте удалит запись, если она есть; это согласуется с дефолтом.
- i18n: новый ключ `general.autostart_label` + ключ заглушки «только Windows» во всех 6 локалях `locales/app.yml`; ключи конфига и тексты логов — English.
- docs-дрейф исправляется: `docs/spec.md` — строка version-log, удаление bullet «Автозапуск при логине» из out-of-MVP (§2), добавление требования в §4.4 (General), ключ `autostart` в схему §7; `README.md:27` — формулировка «без реестра» правится (реестр используется только для автозагрузки по явной опции).

## Constraints

- Стек: Rust edition 2024, bin-only root crate; `serde`+`toml`; GUI `eframe/egui`; i18n `rust_i18n` (один каталог `locales/app.yml`, 6 локалей).
- Конвенции конфига: bool-поле по прецеденту `auto_install` (plain bool, struct-level `#[serde(default)]`, ручная `Default for General`); «save ничего не валидирует» — валидация на стороне GUI; битый конфиг → defaults + ERROR, файл не переписывается.
- `#![forbid(unsafe_code)]` в корневом crate; новый crates.io-деп требует подтверждения пользователя; бюджет exe < 12 MB (spec §5); winreg не добавляет native-пакетов.
- Cross-build `x86_64-pc-windows-gnu` (MinGW) — winreg pure-Rust, собирается; Linux-гейт (`./scripts/check.sh`) должен компилировать всё → cfg-двойки обязательны.
- Тесты: sibling `src/<module>.tests.rs`, naming `<unit>_<scenario>_<expectation>`, pure-logic с инъекцией env; Windows-исполняемые тесты под `#[cfg(windows)]` (исполняются на Windows-раннерах CI и на хосте пользователя); Windows-гейт compile-only: `./cargo.sh test --no-run --target x86_64-pc-windows-gnu`.
- Хук-точки ровно две: `SettingsApp::apply()` и `run_resident` (research.md).

## Open questions

- Подтверждение новой зависимости `winreg` (правило project: new deps require explicit user confirmation) — единственный blocking-вопрос; всё остальное решено автономно (режим «делай автономно»).

## Changes

- 2026-11-13T00:46:29+03:00 -- v1, итоговая спецификация (draft + research): решены открытые вопросы — дефолт `false`; на Linux чекбокс показывается disabled-заглушкой (прецедент поля геймпада), не скрывается; имя значения реестра `virshot`; ошибки автозагрузки только логируются (без toast); тестовый слой = pure-logic на Linux + `#[cfg(windows)]`-тесты; выбор механизма реестра — `winreg` как target-scoped деп с явным утверждением пользователя.
- 2026-11-13T08:32:18+03:00 -- пользователь утвердил зависимость `winreg 0.5.2` (default-features=false, target-scoped cfg(windows)) — открытый вопрос закрыт, скоп/подход не изменились.
- 2026-11-13T08:54:00+03:00 -- факт из research-процедур: актуальная версия `winreg` — 0.56.0 (0.5.x из плана устарела); API: `HKCU.open_subkey_with_flags`, `set_value`/`get_value`/`delete_value`, `ErrorKind::NotFound`. Версия в спецификации — «0.56 по lock», утверждённый вариант депенса (winreg, target-scoped) не изменился.
