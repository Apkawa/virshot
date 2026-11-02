# 2026-11-01_share-button-poc: PoC нестандартных кнопок геймпада (Share и др.)

**Created:** 2026-11-01T20:22:28+03:00

## Scope

- Минимальный PoC: через raw HID-отчёты (`hidapi` crate) — **отладочный вывод всех нажатых кнопок** (down/up), включая те, которых нет в XInput: **Share/Capture**, **View/Back**, **Guide/Xbox**, и все стандартные кнопки как sanity-проверка полного отчёта.
- Проверка на проводном подключении (USB/Xbox Series X|S). Bluetooth — secondary, если повезёт с 17-байтными пакетами.
- PoC ничего не снимает и не интегрируется с текущим event loop `virshot` — только показывает нажатия кнопок.

Out of scope: интеграция в основной loop/tray, конфиг-биндинги, debounce, UI, эмуляция клавиатуры, другие модели геймпадов кроме Xbox-family.

## Approach

- `windows` crate отпадает: требовались `unsafe`-блоки в нашем коде → нарушает `#![forbid(unsafe_code)]` — путь через **`hidapi` crate** (safe API, тот же HID-транспорт, что SDL hidapi использует в remapper).
- Декодирование отчёта — чистая функция (таблица смещений по длине отчёта/типу соединения), покрытая unit-тестами на Linux без железа (TDD).
- Чтение HID — модуль, компилируемый только под Windows (`cfg(windows)`), зависимость hidapi объявляется как `[target.'cfg(windows)'.dependencies]` (Linux-сборка/тесты её не трогают).
- PoC-бинарь: минимальный — перечислить устройства `api.device_list()` и открыть игровые контроллеры по **Usage Page `0x01` + Usage `0x04`/`0x05`** (Windows-бэкенд hidapi заполняет эти поля; VID/PID — вторичный фильтр модели), отправить power-on пакет `05 20 00 01` (как SDL, чтобы пошёл полный state-отчёт), non-blocking read-loop (~10 мс), декодировать bitmask, печатать нажатия.

## Constraints

- `#![forbid(unsafe_code)]` — hidapi внутри допустим (unsafe в third-party), наш код без unsafe.
- Windows build: `cargo build --target x86_64-pc-windows-gnu` (MinGW); Linux `cargo test` остаётся зелёным (target-specific dep).
- Новая зависимость `hidapi` — подтверждена пользователем (2026-11-01, «делаем poc через hidapi»).
- XInput не передаёт Share/View в принципе (спека XInput = набор Xbox 360) — зафиксировано как платформенный факт.
- Смещения Share-бита по данным SDL3 (проводной: size<44→14, 44→18, 46→28, 60→42; BT 17-byte→16) отличаются от «народной» таблицы из snippet пользователя (size<48→18, else→22) → смещения верифицируются эмпирически на реальном железе.
- Windows перехватывает кнопки: Game Recording/Broadcasting — Share; Game Bar — Guide (remapper README.md:12-22). HID-чтение shared, но для PoC желательны отключённые перехватывающие функции.

## Open questions

- Сборка `windows-native` под mingw (`--target x86_64-pc-windows-gnu`) — проверяется на Stage 2.

## Changes

- 2026-11-01T20:22:28+03:00 -- v1, initial spec (draft + research: remapper = SDL3 HIDAPI raw-HID путь; решение пользователя: hidapi, windows crate отклонён из-за forbid(unsafe_code))
- 2026-11-01T20:24:06+03:00 -- пользовательская инфо: идентификация устройств через Usage Page `0x01` / Usage `0x04`/`0x05` вместо хрупкого списка VID/PID; внесено в Approach, VID/PID — вторичный фильтр
- 2026-11-01T20:26:42+03:00 -- пользователь подтвердил: PoC = отдельный scratch-крейт `.tmp/hidpad-poc/` (не трогает deps virshot); проверка = кросс-сборка здесь + unit-тесты декодера на Linux, live-запуск на Windows-хосте пользователя
- 2026-11-01T20:31:39+03:00 -- пользователь уточнил: PoC — отладочный вывод всех нажатых кнопок (не только Share); запуск плана подтверждён
- 2026-11-01T20:36:12+03:00 -- research-уточнение: у `hidapi` нет feature `win32`/`native`; использован `windows-native` (hidapi 2.6.7, pure Rust через windows-sys)
