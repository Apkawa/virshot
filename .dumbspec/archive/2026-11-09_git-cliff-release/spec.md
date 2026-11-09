# 2026-11-09_git-cliff-release: release helper (git-cliff)

**Created:** 2026-11-09T11:36:17+03:00

## Scope

**In scope:**
- git-cliff как dev-инструмент проекта: установка через `cargo install`,
  целевая версия зафиксирована в проекте. NOT a crate dependency.
- `cliff.toml` — конфиг git-cliff в корне репозитория (changelog + bump).
- `scripts/release.sh` — тонкий wrapper:
  - `release.sh changelog` — генерация `CHANGELOG.md` (файл, без коммита);
  - `release.sh minor` / `release.sh patch` — ZeroVer bump версии,
    коммит `chore(release): prepare for v0.1.2`, git tag `v0.1.2` на этом
    коммите;
  - `release.sh major` — запрещён (ошибка), до решения «всё готово».
- `CHANGELOG.md` в корне репозитория.

**Out of scope:**
- CI (никаких pipeline/workflow).
- `major` bump (запрещён в ZeroVer).
- pushing тегов/коммитов в remote — скрипт работает локально, push руками.

## Approach

- git-cliff, пин **2.14.2** (`./cargo.sh install git-cliff --version 2.14.2`,
  sandbox CARGO_HOME в `./.cache/`). Бинарь dev-инструмента, не crate-деп.
- `cliff.toml` (на базе дефолтного шаблона `--init`, правки под проект):
  - `[git] conventional_commits = true`, `tag_pattern = "v[0-9].*"`;
  - ZeroVer через `[bump]`: `features_always_bump_minor = false`,
    `breaking_always_bump_major = false`, `initial_tag = "v0.1.0"`
    (тегов в репо нет — ноль тегов → стартовая версия из `initial_tag`).
    Эмпирика 2.14.2: `initial_tag` обязан совпадать с `tag_pattern`, поэтому
    `"v0.1.0"`, а не `"0.1.0"`; следствие — ПЕРВЫЙ релиз публикует текущую
    версию (`v0.1.0`) независимо от minor/patch, коммит-дривен bumps
    работают только после появления tag'а.
  - В ZeroVer `major` физически не возникает при major == 0; subcommand
    `major` в скрипте запрещён явно — две защиты.
- `scripts/release.sh` (стиль существующих скриптов: `#!/usr/bin/env bash`,
  `set -euo pipefail`, root через `BASH_SOURCE`, комментарии EN):
  1. вычисляет следующую версию через git-cliff (`--bump <minor|patch>` —
     явный тип принимается только `--bump`; retrieval-инвокация проверяется
     при реализации против установленного бинаря);
  2. записывает версию в **root `Cargo.toml`** (bump scope — только root
     `virshot`; crate-версии не бампаются);
  3. перегенерирует `CHANGELOG.md` полностью: `git cliff --tag "v<ver>" -o
     CHANGELOG.md` (паттерн release.sh самого git-cliff);
  4. `git add` + коммит `chore(release): prepare for v<ver>` (дефолтный
     cliff.toml пропускает такие коммиты из changelog);
  5. аннотированный tag `git tag -a v<ver> -m "Release v<ver>"` (без
     GPG-подписи — ключа в проекте нет).
  - `changelog` subcommand: только `git cliff -o CHANGELOG.md`, без commit.
- git-cliff сам НЕ пишет версию в Cargo.toml и НЕ создаёт tag — шаг 2 и 5
  делает скрипт (это факт 2.14.2, из research.md).

## Constraints

- Rust workspace: root `virshot 0.1.0` + crates overlay/toast/gamepad/icons/
  video-capture (все `0.1.0`); 28 коммитов, все Conventional Commits;
  теги отсутствуют (первый релиз).
- git-cliff не установлен; MSRV 1.88.0 ≤ rustc 1.99.1; установка только через
  `./cargo.sh install` (sandbox).
- Коммит релиза прогоняет rusty-hook pre-commit gate (rare event, accepted).
- Shell-конвенции репо: `set -euo pipefail`, `BASH_SOURCE`-root, cargo только
  через `./cargo.sh`, комментарии EN.
- В 2.14.2 нет `zerover`/`version_bump_type`/`--bump-version`/`tag_prefix` —
  используются `[bump]`/`--bump`/`tag_pattern`.

## Open questions

none — решения пользователя 2026-11-09: bump только root `virshot`; пин
git-cliff 2.14.2; дефолтный шаблон changelog. Осталось только не-блокирующее
подтверждение при реализации: явный тип `minor|patch` в `--bumped-version`.

## Changes

- 2026-11-09T11:36:17+03:00 -- v1, initial spec (draft + research; решения
  пользователя: tag `v0.1.2`, bump = коммит + tag, `major` запрещён,
  git-cliff через cargo install с фиксацией версии)
- 2026-11-09T15:01:03+03:00 -- решения пользователя по review: bump версии
  только в root `Cargo.toml` (crates не бампаются); git-cliff пинится на
  2.14.2; changelog — дефолтный шаблон git-cliff
- 2026-11-09T15:33:36+03:00 -- execution-эмпирика: `initial_tag = "0.1.0"` не
  проходит валидацию 2.14.2 (не совпадает с `tag_pattern`) → `initial_tag =
  "v0.1.0"`; первый релиз публикует текущую версию v0.1.0, bumps работают
  только после tag'а; явный тип bump'а — через `--bump <type>`
  (`--bumped-version` — value-less flag)
