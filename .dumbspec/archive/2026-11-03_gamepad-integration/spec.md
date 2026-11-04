# 2026-11-03_gamepad-integration: gamepad-библиотека и интеграция в virshot

**Created:** 2026-11-03T22:13:26+03:00

## Scope

**In scope:**
1. `crates/gamepad/` — standalone-библиотека, скопированная из `hidpad-poc/` почти 1:1
   (минимальные изменения): COM-shim gameinput.dll, layout-структуры, vendor-заголовок,
   публичный API поверх poc-механики, консольное тестовое приложение в `examples/`.
2. Интеграция в virshot (только Windows): поток-помпа gameinput, сопоставление
   бинда `pad` из config (кнопки + Guide/Share system-кнопки) с debounce 250 ms,
   вызов существующего `capture::spawn_capture`; UI-бинд «press a button»
   (захват нажатия) вместо placeholder; метки кнопок через `GetButtonLabel`.
3. Обновление `docs/spec.md` и `AGENTS.md` под факт интеграции.

**Out of scope:**
- Linux-бэкенд (gilrs) — нет: hardware-находка (константный timestamp в wgi,
  нажатия не приходят, vendor=1118 product=2835). Crate компилируется на Linux
  (layout + API без реализации), интеграция в virshot под `#[cfg(windows)]`.
- Аналоговые оси/триггеры/вибрация — только кнопки.
- Мультикнопочные комбо — один бинд на действие (по docs/spec.md §4.1).
- Эмуляция клавиатуры — нет.

## Approach

- **Форма библиотеки:** каталог `crates/gamepad/` — копия poc со своим
  `Cargo.toml` (zero-deps), не workspace-member (в root нет `[workspace]`);
  virshot подключает её как path-зависимость (`gamepad = { path = "crates/gamepad" }`).
  `src/main.rs` poc → `src/lib.rs` facade (pub API: init/registration, pump,
  device registry, кнопка-state-diff события, Guide/Share system-кнопки);
  `gameinput.rs`/`layout.rs` переносятся почти 1:1; `vendor/GameInput.h` + NOTICE
  копируются; `build.rs` со stamp — не нужен в библиотеке.
  `examples/buttons.rs` — консольное приложение на API библиотеки (вывод в стиле
  poc-лога: device lines, `button bits=... idx=[...]`, guide/share DOWN/UP, heartbeat).
- **Не добавляем crate `windows`:** poc-доказывает, что GameInput работает
  hand-written shim'ом без зависимостей — решение «windows crate RawGameController»
  из AGENTS.md/docs/spec.md заменяется shim-ом; новых зависимостей нет, approval не нужен.
- **unsafe:** в virshot сохраняется `#![forbid(unsafe_code)]` (src/main.rs);
  в gamepad — паттерн poc: crate-root `#![deny(unsafe_code)]` + единственное
  исключение `#![allow(unsafe_code)]` в `gameinput.rs` (COM-vtable нельзя
  смIRRORить без unsafe).
- **Механика без изменений:** focus policy 0x1C0 перед регистрацией; QI-негосиация
  v3→v2→v0; dispatcher `Dispatch(5000µs)` pump + 50 ms button poll (state-diff);
  kind-фильтры 0x0E/0x40000/0x04 async + blocking Connected; heartbeat 5 s.
- **Bind-формат `pad`:** расширяем нотацию в config: канонические имена
  (`South`, `North`, `East`, `West`, ...) → controller button index
  (маппинг по GameInput-порядку, реализован в gamepad-модуле virshot);
  `unknown:<N>` → raw index из захвата «press a button»; `Guide`, `Share` →
  system-buttons (обработываются через RegisterSystemButtonCallback).
  Действие срабатывает по любому заполненному полю (OR), debounce 250 ms
  (тот же `bindings::Debounce`).
- **Поток:** отдельный thread «gamepad pump» (виршот §6: gamepad poll thread),
  события → listener-обработка → debounce → `capture::spawn_capture` (owned values,
  без shared-Mutex триггерного состояния).
- **UI:** pad-поле кликом входит в режим «press a button»: первые DOWN-edge после
  клика пишут `unknown:<N>` (или `Guide`/`Share` для system-кнопок); метка
  `GetButtonLabel` показывается как hint. Connected-list не делаем (YAGNI).
- `hidpad-poc/` остаётся как PoC (не трогаем); дублирование кода между PoC и
  библиотекой допустимо (poc — исторический артефакт).

## Constraints

- Rust edition 2024; gates: `./scripts/check.sh` (linux check+test+clippy) и
  `./cargo.sh test --no-run --target x86_64-pc-windows-gnu`; коммит через rusty-hook chain.
- `#![forbid(unsafe_code)]` в virshot (src/main.rs:57); deny/allow-паттерн в gamepad.
- Одна модуль = один файл; unit-тесты sibling `src/<module>.tests.rs`; TDD для unit-логики.
- Trigger-хук: `capture::spawn_capture` (src/capture.rs:149); debounce в `src/bindings.rs`.
- GameInput-ABI авторитет: `vendor/GameInput.h` (46 378 B) — слоты и offset по
  строкам заголовка; layout-тесты фиксируют offsets.
- `GetButtonLabel` в poc не реализован — добавляем как slot-вызов в библиотеке
  (единственное содержательное добавление сверх 1:1).
- Share в live-логе не наблюдался (Game Bar перехватывает); требует hardware-verification на Windows.
- Кэш-конвенции: `./cargo.sh` (CARGO_HOME=./.cache/cargo); scratch в `./.tmp/`.

## Open questions

- none (решения приняты на review: shim вместо windows-crate; bind-формат
  canonical|unknown:<N>|Guide|Share; dispatcher+poll как в poc; Linux вне scope;
  `crates/gamepad` как location).

## Changes

- 2026-11-03T22:10:10+03:00 -- пользователь добавил: тестовое консольное приложение
  в `crate/gamepad/examples/`
- 2026-11-03T22:13:26+03:00 -- v1, initial spec (draft + research + review decisions:
  hand-written shim вместо `windows` crate, bind-формат `pad`, Linux out of scope)
- 2026-11-03T23:38:05+03:00 -- execution finding (Stage 2): в vendored header НЕТ
  слота `GetButtonLabel` на `IGameInputDevice` (это имя WinRT); метки читаются из
  device-info блока (v3 controllerInfo→controllerButtonLabels, v2 controllerButtonInfo,
  v0 — None). Библиотека экспортирует `button_label(device_index, button_index)` +
  `button_name(GameInputLabel code)`. Также в заголовках нет `GameInputButtonFlags`:
  маппинг канонических имён → индекс слота в Stage 1 проверен только тестами-таблицей,
  требует hardware-verification через пример `buttons`.
- 2026-11-04T00:32:38+03:00 -- execution note (Stage 4, commit `007aeb7`): один
  shared debounce-гейт 250 мс на оба источника (`bindings::try_fire_shared`,
  `Arc<Mutex<Debounce>` в main.rs) — OR-fire без double-fire. Найденные дефекты,
  перенесённые в Stage 5: снимок Settings в PadObserver (UI Apply не доходит до
  pad-выстрелов — нужен mailbox как в hotkeys), fire по 0→1 переходу в `bits`
  (pressed_indices = текущее состояние), отсутствие события device_disconnected
  в crate. Hardware-verification (Guide/Share) остаётся отложенным.
- 2026-11-04T01:55:00+03:00 -- execution note (Stage 5, commit `d338190`: UI capture
  `unknown:<N>`/`Guide`/`Share`, settings mailbox hotkeys/pump/GUI, prev-mask
  0→1 tracker, crate `device_disconnected`, shot-suppression при capture):
  Escape НЕ отменяет pad-capture (отмена — повторный клик / старт keyboard
  capture); post-commit гонка одного кадра; pump-init failure на Windows
  оставляет поле в capture без таймаута — notes для hardware-run.
- 2026-11-04T09:42:08+03:00 -- Stage 6 (docs): `docs/spec.md` v5 — геймпад
  реализован на Windows, Linux gilrs вне scope (hardware-finding vendor=1118
  product=2835), Share/Guide отложены (§9); `AGENTS.md` — path dep
  `crates/gamepad`, ноль crates.io deps, stale «windows via winit» исправлен
  (windows transitive через xcap 0.9.8); README stale-строки исправлены;
  gui/gamepad doc-комменты приведены к факту (device-info label hint).
- 2026-11-04T09:57:00+03:00 -- settlement: пользователь проверил `buttons.exe` на
  железе — «все ок»; hardware-acceptance examples закрыта, остаток (virshot с
  pad-биндом на Windows) документирован в docs/spec.md §9; задача архивирована
  по подтверждению пользователя.
