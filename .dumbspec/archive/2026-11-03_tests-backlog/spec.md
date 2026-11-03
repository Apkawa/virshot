# 2026-11-03_tests-backlog: перенести задачи из бэклога (раздел tests) в драфт

**Created:** 2026-11-03T16:38:56+03:00

## Scope

Три задачи из бэклога (`docs/spec.md`, раздел tests) становятся рабочим планом.

1. **Юнит-тесты — sibling-файлы.** 232 юнит-теста (231 pass + 1 ignored), сейчас живущих в
   `#[cfg(test)] mod tests` внутри 10 модулей `src/`, переносятся в отдельные файлы
   `src/<module>.tests.rs` (`bindings`, `capture`, `config`, `gui`, `hotkeys`, `instance`,
   `logging`, `overlay`, `template`, `tray`). В модуле остаётся только хук:
   `#[cfg(test)] #[path = "<module>.tests.rs"] mod tests;`. Тесты остаются теми же
   ассертами — перенос без переписывания логики.
   Решение пользователя 2026-11-03: **sibling-стиль вместо `tests/unit/`** — конвенция
   AGENTS.md («тесты модуля в его файле или в sibling-файле») сохраняется, видимость
   `pub(crate)`/приватных items не расширяем (child-модуль видит приватные items родителя),
   `src/lib.rs` не добавляем.
2. **Ручной тест приложения под дисплеем — скрипты в `./scripts/` (только Linux).**
   Цель: агент может «видеть» приложение — снимок окна настроек, снимок всего экрана,
   лог, факт захвата по горячей клавише. Это helper-скрипты для ручного прогона, **не
   E2E-тесты**: `tests/cli_headless.rs` остаётся как есть, новых тестов не добавляем.
3. **Профилирование — рабочий процесс, не оптимизации.** Настроить и проверить
   прогоны: CPU-сэмплинг (`perf record --call-graph dwarf`), куча (`valgrind
   --tool=massif`), утечки/ошибки памяти (`valgrind --tool=memcheck`), счётчики
   memory-событий (`perf stat`). Результаты — артефакты в `./.tmp/` + короткий
   документ «как читать». Оптимизации по результатам — out of scope.

Out of scope: новые E2E-тесты (решение 2026-11-03); `src/lib.rs` и расширение видимости;
оптимизации по профилям; Windows-специфичные хелперы; изменения функциональности
приложения ради тестов; окно-режим захвата приложения (`shot --window`) как цель хелперов.

## Approach

- **Тесты.** Для каждого модуля — `src/<module>.tests.rs` + хук в модуле. Тесты
  переезжают текстом как есть (имена `<unit>_<scenario>_<expectation>` сохраняются).
  Gate: `./scripts/check.sh` (linux) и `./cargo.sh test --no-run
  --target x86_64-pc-windows-gnu`. `cargo fmt` — только на границе этапа
  (`./scripts/check_full.sh` / коммит).
- **Скрипты ручного теста.** Путь X11/Xvfb — единственный наблюдаемый путь на этом
  хосте (Wayland-захват невозможен, см. research R2b/R2c/R2d). Один вызов bash = один
  скрипт: `xvfb-run -a --server-args="-screen 0 1280x1024x24"` + `env -u
  WAYLAND_DISPLAY`, внутри `dbus-run-session --` (трей регистрируется на session bus),
  изолированные `--config`/log/`save_path` в `./.tmp/`, запуск резидентного `virshot`,
  ожидание окна через `xdotool search --name virshot`, снимок окна
  `import -window <id>` (или `scrot -w`) и root-снимок `scrot -o`, ввод
  `xdotool key Print`, проверка лога («Hotkey registered», «Saved:», «Toast shown»),
  остановка по SIGTERM и удаление `<config>.lock`. `windowkill` не используем как
  корректную остановку (оставляет stale lock).
  **Трей** наблюдаем через D-Bus, а не пикселями: `tint2` не линкует libdbus и не
  предоставляет `StatusNotifierWatcher` → SNI-иконка в панели не рисуется (в панели
  виден только taskbar). Evidence: bus-имя `org.kde.StatusNotifierItem-<pid>-1`,
  `ToolTip`/`IconPixmap` (32×32 RGBA → PNG), `/MenuBar` layout (`Take screenshot`,
  `Settings`, `Exit`), `Activate`, menu `Event` (Settings показывает окно, Exit
  корректно завершает процесс и снимает lock).
  `openbox` — опция `--wm`: добавляет EWMH (`wmctrl -l/-m`, `getactivewindow`) и режим
  `shot --window` приложения; там кадрируем по **absolute** origin (окно реparentится,
  тайтлбар openbox даёт смещение 1,20) и НЕ проверяем регистрацию хоткеев (под WM
  регистрация падает с «HotKey already registered», причина не pinned — loop пропущен
  по решению пользователя 2026-11-03). Без WM регистрация успешна, поэтому ассерты
  хоткеев — в конфигурации без WM.
- **Профилирование.** Хелперы в `./scripts/`: CPU-профиль резидентного/циклического
  прогона (`perf record`, анализ `perf report --stdio` / `perf script`), heap-профиль
  headless-пути (`valgrind --tool=massif --time-unit=ms --massif-out-file=./.tmp/…`),
  `memcheck` на CLI-пути, `perf stat -e mem_inst_retired.all_loads,…` как счётчики.
  `perf mem` не используем (WSL2 PMU не поддерживает sampling). SVG не генерируем:
  `flamegraph`/`inferno` не ставим, профиль читается текстом
  (`perf report --stdio`, `perf script`). Профилируем debug-бинарь.

## Constraints

- Crate сейчас **binary-only** (`src/main.rs` root, `#![forbid(unsafe_code)]`): интеграционные
  тесты не видят `pub(crate)`/приватные items — отсюда sibling-стиль вместо `tests/unit/`.
  Тестируемые items: `pub(crate)` + приватные helpers (232 теста, `template.rs:197`,
  `instance.rs:188`, `config.rs:516`, `gui.rs:1466`, `hotkeys.rs:505`, `capture.rs:423`,
  `overlay.rs:180`, `bindings.rs:263`, `logging.rs:184`, `tray.rs:246`).
- Gates: `./scripts/check.sh` (check + test + clippy -D warnings), `./scripts/check_full.sh`
  (+ `cargo fmt`/`--check`); windows-gnu `test --no-run` компилирует и `cfg(test)`-код.
- Окружение: WSL2; `/tmp` fresh на каждый bash-вызов (Xvfb+приложение+захват в одном
  вызове); `xvfb-run -a` работает без `mkdir /tmp/.X11-unix`, bare `Xvfb :9` — не
  создаёт сокет без mkdir. Установлены: `xvfb`/`Xvfb`, `xdotool` 3.x, `scrot` 1.10,
  `grim` 1.4, `weston` 13, `wmctrl` 1.07, `openbox`, `tint2`, D-Bus инструменты
  (`dbus-run-session`, `dbus-daemon`, `busctl`, `gdbus`, `dbus-send`), ImageMagick,
  `perf` 6.18 (собран пользователем, `/usr/local/bin/perf`), `valgrind` 3.22.
  Нет: `flamegraph`/`cargo-flamegraph`, `inferno`, `msprint`, `massif-visualizer`.
  `tint2` не линкует libdbus и не предоставляет `StatusNotifierWatcher` → SNI-трей на
  этом хосте визуально не observable (только D-Bus evidence).
- `perf_event_paranoid=2` → CPU-сэмплинг своего процесса без root. **`perf mem` / PEBS не
  работают** (WSL2 PMU); memory-события только через `perf stat`.
- `[profile.release]`: `opt-level="z"`, `lto="thin"`, `strip=true` → релиз без символов;
  профилируем debug-бинарь (или отдельный непрофилирующий профиль по согласованию).
- Конфиг-готча: `save_path` в `[general]` (не `output_dir`); дефолтный путь
  `~/Pictures/Screenshots` в песочнице read-only → всё в `./.tmp/`.
- Захват xcap под Xvfb нестабилен: `virshot shot` 3/8 и 2/10 успешных; `scrot` 5/5 →
  в скриптах нужен retry-loop для xcap-пути.
- Новые пакеты/криейты требуют явного подтверждения пользователя (AGENTS.md); временные
  артефакты — `./.tmp/`, кэш — `./.cache/`.

## Open questions

Решены пользователем 2026-11-03 (см. Changes):

1. SVG-рендер — **не ставим** (`flamegraph`/`inferno` не устанавливаем); CPU-профиль
   читается текстом: `perf report --stdio` / `perf script`.
2. Профилируемый бинарь — **debug** (`target/debug/virshot`, символы есть);
   `Cargo.toml` не меняем, `[profile.profiling]` не добавляем.
3. Форма helper-скриптов — **один `scripts/ui-verify.sh`** с субкомандами
   (`start`, `shot`, `window`, `log`, `stop`) внутри одного Xvfb-вызова.
4. Workload для профилирования (фиксируем в плане): куча/утечки — headless-путь
   (`virshot shot` в цикле, massif даёт отчёт); CPU — резидентный прогон под
   `perf record` с остановкой по SIGTERM и удалением `<config>.lock`.

none outstanding.

## Changes

- 2026-11-03T16:38:56+03:00 -- v1, initial spec (draft + research R1/R2/R2b/R2c/R3/R3b)
- 2026-11-03T15:21:00+03:00 -- пользователь переформулировал п.2: вместо E2E-тестов —
  helper-скрипты в `./scripts/` для ручного теста; главное — агент может видеть
  приложение; E2E пока не делаем
- 2026-11-03T15:26:00+03:00 -- пользователь установил `xvfb`, `xdotool`, `scrot`, `grim`,
  `weston`; позже докинул `wmctrl` (WM/openbox — нет): путь «видеть приложение» стал
  рабочим через Xvfb + `import -window <id>`/`scrot -w`
- 2026-11-03T16:07:32+03:00 -- решение по п.1: sibling `src/<module>.tests.rs` вместо
  `tests/unit/` (без `src/lib.rs` и без расширения видимости)
- 2026-11-03T16:07:32+03:00 -- пользователь собрал `perf` и поставил `valgrind`: п.3
  строится на `perf record`/`perf stat` + `valgrind massif`/`memcheck`; `perf mem` и
  `flamegraph --memory` недоступны на этом хосте
- 2026-11-03T19:09:51+03:00 -- пользователь установил `tint2` и `openbox`; п.2 дополнен:
  трей наблюдаем через D-Bus (`StatusNotifierItem`, `IconPixmap`, `/MenuBar` layout,
  `Activate`, menu `Event`), `openbox` — опция `--wm` (EWMH + `shot --window`);
  `tint2` не рендерит SNI-иконки (нет libdbus/watcher); хоткей-ассерты — без WM
- 2026-11-03T19:09:51+03:00 -- ресерч-цикл по tint2/openbox (почему хоткеи падают под WM) пропущен
  по решению пользователя; триггер не pinned, зафиксирован как gap в research R2d
- $(date -Isecond) -- Stage 1–3 исполнены и проверены: 232 юнит-теста в sibling-файлах,
  `scripts/ui-verify.sh` (окно/root-захват, hotkey-захват, трей через D-Bus, опция `--wm`),
  `scripts/prof.sh` (`heap|leak|cpu|mem-count`); peak heap ~10.1 MiB, `definitely lost: 0`,
  CPU-профиль без SVG; hotkey-ассерты только без WM (под openbox PrintScreen занят WM)
