# 2026-11-09_github-ci: GitHub CI — тестирование и сборка релиза

**Created:** 2026-11-09T19:05:06+03:00

## Scope

**Внутри:**
- GitHub Actions, два workflow:
  1. **`ci.yml` — тестирование**: на push в `master` и на PR. Linux: установка
     native-dev-пакетов, `cargo fmt --check`, `cargo check --workspace --all-targets`,
     `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`.
     Windows (`windows-latest`): полный прогон `cargo test --workspace` + clippy.
  2. **`release.yml` — сборка релиза**: две цели
     `x86_64-pc-windows-msvc` и `x86_64-unknown-linux-gnu` (`aarch64-pc-windows-msvc`
     — вне scope этой задачи, см. «Вне scope»); артефакты, `release.yaml`,
     changelog в тело релиза, публикация на GitHub.
- Имена артефактов (короткие токены):
  `virshot-v<version>-win-x64.zip`, `virshot-v<version>-linux-x64.tar.gz`.
- `release.yaml` прикрепляется к релизу: `version`, `releaseDate` (ISO-8601, ms, `Z`),
  `commit_sha` (короткий sha), `files[]` с `url` (полный download-URL), `type`
  (target triple), `sha512` (base64 от 64-байтового дайджеста), `size` (байты).
- **Prerelease**: один rolling-релиз с тегом `nightly`, обновляется in-place на любой
  коммит в `master` (старые ассеты удаляются, новые загружаются). Имена prerelease-
  артефактов: `virshot-v<version>-nightly-<shortsha>-win-x64.zip` и т.д.
- В тело релиза — changelog, накопившийся между релизами, из `git-cliff` (stdout).
- В архив — только бинарь (`virshot.exe` / `virshot`).

**Вне scope:** macOS и другие цели, сборка `aarch64-pc-windows-msvc` (`win-arm64`) —
выведена из scope решением пользователя от 2026-11-09T20:05+03:00 (будет отдельной
задачей, когда понадобится), MinGW-артефакты (`windows-gnu` — остаётся
локальной сборкой), инсталлятор, автообновление, подпись кода, external/third-party
Actions (используем только официальные GitHub actions + `gh` CLI), сборка релиза из PR.

## Approach

- **Триггеры `release.yml`**: push тега `v*` → полноценный релиз; push в `master` →
  rolling prerelease `nightly`; `workflow_dispatch` — ручной перезапуск.
- **Сборка целей**: каждая цель собирается на своём runner (release-профиль):
  Windows-цель (`x86_64-pc-windows-msvc`) на `windows-latest` (MSVC-линкер доступен только там — из Linux линковка
  `link.exe` не проходит), Linux-цель на `ubuntu-latest`. Джобы отдают **бинарь**
  как workflow-artifact; упаковка и публикация — один финальный job на
  `ubuntu-latest`: скачивает бинари, пакует zip/tar.gz, считает `sha512` (base64) и
  `size`, пишет `release.yaml`, публикует релиз через `gh` CLI.
- **Changelog**: `git-cliff` закрепляет версию 2.14.2; в CI бинарь ставится
  `cargo install git-cliff --version 2.14.2 --no-default-features` (git-cliff остаётся
  внешним dev-инструментом, не зависимостью — решение пользователя). Тело релиза:
  для tag-релиза — интервал между последними тегами (`git-cliff --latest`/`--tag`,
  проверяется empirically), для prerelease — `git-cliff --unreleased -o -` (stdout).
- **Версия**: берётся из `Cargo.toml` (корневой `[package] version`); bump и tag
  по-прежнему делает `scripts/release.sh` локально (он не пушит — tag пушится
  вручную), CI ничего не bump-ит. В prerelease `version` в `release.yaml` = версия из
  `Cargo.toml`, различие builds — по `commit_sha` и флагу prerelease.
- **Публикация prerelease in-place**: GitHub release требует `tag_name`; тег
  `nightly` не подходит под `tag_pattern = "v[0-9].*"` в `cliff.toml`, поэтому
  git-cliff не считает его релизом. Схема: создать релиз `nightly` (`--prerelease`),
  далее удалять старые ассеты (`gh release delete-asset`) и загружать новые
  (`gh release upload ... --clobber`), обновлять тело/`--target-commit`.
- **Тестирование в CI**: e2e на runner работает headless по конструкции
  (проверяет путь отказа, success-тест `#[ignore]`); на Windows полный прогон
  проверяется empirically и при проблемах деградирует до `--no-run` (решение этапа).
- **CI не копирует sandbox-специфику**: не использует `./cargo.sh`, `.cache/`,
  `.tmp/` как кэши; `rusty-hook` на CI не ставит hooks (его build.rs сам
  пропускает CI), поэтому гейты воспроизводятся прямо в workflow.

## Constraints

- Репо: `github.com/Akawa/virshot` (remote `origin`); `.github/` отсутствует
  полностью — workflows создаются с нуля.
- Rust stable (`rust-toolchain.toml: channel = "stable"`), edition 2024; локально
  1.99.1. Targets локально: linux-gnu, windows-gnu, windows-msvc x64;
  `aarch64-pc-windows-msvc` не установлен (эта цель вне scope — см. «Вне scope»).
- Linux-сборка требует dev-пакеты: `pkg-config libegl1-mesa-dev libgbm-dev
  libudev-dev libwayland-dev libpipewire-0.3-dev libclang-dev` (CONTRIBUTING.md).
- Windows-цели не требуют native-пакетов (`xcap` на Windows — чистый Rust).
- `[profile.release]`: `opt-level="z"`, `lto="thin"`, `strip=true`.
- `.cargo/config.toml` полностью закомментирован (MinGW-пиннинг не активен) — для
  MSVC-целей в CI это не мешает, линкер берётся от MSVC toolchain.
- Локальные гейты (`scripts/check.sh`, `scripts/check_full.sh`) — root package; CI
  использует `--workspace --all-targets`. `cargo fmt` в CI только `--check`
  (не rewrite).
- Измеренные размеры: Windows exe 8.37 MB (бюджет spec §5 <10 MB соблюдён), Linux
  release-бинарь 15.1 MB (бюджет §5 в `docs/spec.md` описан для Windows-поставки).
- git-cliff 2.14.2 закреплён как dev-инструмент (не crate); в CI ставится отдельно.
- Новые crates.io-зависимости не добавляются (по AGENTS.md требуют approval).
- `sha512` в примере запроса = base64 от сырого дайджеста (88 символов — проверено:
  `openssl dgst -sha512 -binary | openssl base64 -A`).
- Тесты: 652 passed / 2 ignored в workspace; e2e `cli_headless::` требует
  записываемого scratch-каталога на runner.

## Open questions

- ~~ feasibility `aarch64-pc-windows-msvc` на `windows-latest` ~~ **закрыто
  2026-11-09T20:05+03:00**: цель выведена из scope этой задачи (решение пользователя —
  «когда надо будет ARM-сборка, тогда и сделаем»). Deep-research (research.md §12):
  цель Tier 1 с host tools, rustup-компоненты доступны; кросс x64→ARM64 на GitHub
  Windows-runner подтверждён практикой; crate-сет репо компилируется на ARM64 (нет
  x86-интринсиков и `asm!`); запуск ARM64-бинаря требует отдельной проверки
  (нативный runner `windows-11-arm`, баг rust-lang/rust#165346 на 1.99.1).
- Поведение полного прогона тестов на Windows runner (headless e2e, tray/hotkey) —
  empirically на этапе CI.
- Бюджет «<10 МБ» для Linux-артефакта (15.1 MB): считаем Windows-бюджет, для Linux
  — без лимита; подтверждение желательно.
- Тело релиза для tag-релиза: точная команда git-cliff для интервала между тегами
  (`--latest` vs `--tag <ver> -o -`) проверяется empirically на этапе.

## Changes

- 2026-11-09T19:05:06+03:00 -- v1, initial spec (draft + research). Решения
  пользователя: имена артефактов — короткие токены (`win-x64` / `win-arm64` /
  `linux-x64`); в архив только бинарь; prerelease — один rolling-релиз in-place
  (тег `nightly`, sha в имени артефакта); триггеры — push тега `v*` + push `master`
  (+ manual dispatch); Windows CI — полный прогон тестов; в `release.yaml` поле
  `url` — полный download-URL (расхождение с примером в запросе, где url = имя
  файла); git-cliff остаётся внешним инструментом, в CI ставится через
  `cargo install` (не добавляется в dev-dependencies).
- 2026-11-09T20:05:33+03:00 -- v2, scope-уточнение пользователя: сборка
  `aarch64-pc-windows-msvc` (`win-arm64`) выведена из scope этой задачи; целевые
  цели — `x86_64-pc-windows-msvc` (`win-x64`) и `x86_64-unknown-linux-gnu`
  (`linux-x64`). Схема имён артефактов и `release.yaml` не меняется, `win-arm64`
  добавится отдельной задачей позже. Deep-research по ARM64 зафиксирован в
  research.md §12.
