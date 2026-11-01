# 2026-11-01_poc-gamepad-screenshot: минимальный POC (геймпад + скриншот)

**Created:** 2026-11-01T12:34:26+03:00

## Scope

**Входит:**
- Инициализация Rust-проекта в репозитории `vibe-shot`: `Cargo.toml`, `.gitignore`,
  `.cargo/config.toml`, `src/main.rs`.
- Консольный POC: опрос геймпада через `gilrs`, печать в консоль подключений
  геймпадов и нажатий кнопок; при нажатии **любой** кнопки — захват экрана
  (`xcap`) и сохранение PNG в `./.tmp/screenshots/` (в отдельном потоке).
- Сборка Linux (`x86_64-unknown-linux-gnu`) и кросс-сборка Windows
  (`x86_64-pc-windows-gnu`) из текущего WSL-окружения.
- Windows `.exe` — пользователь потыкает его на Windows; дальше двинемся к MVP.

**Не входит:** GUI/eframe, оверлей, трей, конфиг, клавиатурные хоткеи, CLI
`vibeshot shot`, jpg/webp, шаблоны токенов, single-instance — всё это из
`docs/spec.md` (MVP), после приемки POC.

## Approach
- Один бинарь, name `vibe-shot` (в MVP переименуем в `vibeshot`).
- Loop из примера пользователя: `gilrs.next_event()` + `sleep(10ms)`,
  захват в `thread::spawn`, имя файла `screenshot_<timestamp>.png`.
- Все `ButtonPressed` печатаются в консоль; **каждое** — триггер скриншота
  (debounce в POC не делаем: это MVP-фича).
- `gilrs` с `default-features = false, features = ["xinput"]` — консольное
  приложение без окна не получает событий через дефолтный WGI (research B).
- Скриншоты — primary монитор (`Monitor::all()` → первый/primary).

## Constraints (из research)
- rustc/cargo 1.99.1, rustup 1.26.0; target `x86_64-pc-windows-gnu` **установлен**;
  MinGW-w64 **не установлен** (нужен linker `x86_64-w64-mingw32-gcc`).
- В sandbox нет sudo → установку пакетов выполняет пользователь:
  `sudo apt install -y mingw-w64` (Windows),
  `sudo apt install -y libudev-dev libwayland-dev libpipewire-0.3-dev libclang-dev` (Linux).
- Сборки в sandbox: `CARGO_HOME=/home/apkawa/code/vibe-shot/.cache/cargo`
  (globally `~/.cargo` read-only).
- Версии: gilrs 0.11.2, xcap 0.9.8, image 0.25.10 (PNG из коробки), chrono 0.4.45.
- `cargo check --target x86_64-pc-windows-gnu` уже проверен — деппы компилируются,
  блокируется только линковка (нет MinGW). Linux-компиляция сейчас падает на
  dev-пакетах (wayland/libudev/pipewire/clang).
- mingw-w64 из apt — 11.0.1 (ниже официально тестируемого 12.0); если `ld`
  ошибится — более новый MinGW (MSYS2) как fallback.
- Риск: xcap windows-gnu линковка end-to-end ещё не проверена (компиляция — да).

## Open questions
- Поведение скриншота в WSL (захват экранов Windows через WSLg) — не критично:
  приемка POC — на Windows, user потыкает `.exe`.

## Changes

- 2026-11-01T12:34:26+03:00 -- v1, initial spec (draft + research); уточнения
  пользователя: триггер = любая кнопка, windows-gnu (MinGW), путь сохранения
  `./.tmp/screenshots/`.
