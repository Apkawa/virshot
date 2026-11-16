# 2026-11-16_ci-optimization: оптимизация CI

**Created:** 2026-11-16T11:01:31+03:00

## Scope

- Оптимизация GitHub Actions в единственном workflow `ci.yml`:
  1. `git-cliff` в job `release` — скачивать готовый бинарник из binary
     releases (pin 2.14.2) вместо `cargo install` (сборка ~десятки минут);
     с проверкой целостности через `.sha512` из тех же releases.
  2. Кеш сборки — `Swatinem/rust-cache` в cargo-job'ах CI (`linux`,
     `windows`, `build`).
- Out of scope: изменение самих гейтов (fmt/check/test/clippy), локальных
  скриптов (`scripts/release.sh` остаётся с локальным бинарём), добавление
  crate-зависимостей, документация (`AGENTS.md`/`CONTRIBUTING.md` описывают
  старый `release.yml` — drift, не правим в этой задаче), PGP-верификация
  `.sig`.

## Approach

- **git-cliff бинарник** (job `release`, ubuntu-latest):
  - вариант **gnu** (`x86_64-unknown-linux-gnu`): runner ubuntu имеет glibc,
    gnu-сборка проверенно работает; musl (static) тоже работает, но gnu —
    выбор из draft и docs не дают причин для musl;
  - step `Install git-cliff` заменяется на step `Download git-cliff`:
    `wget` tar.gz + `.sha512` из `github.com/orhun/git-cliff/releases/download/v2.14.2/`,
    верификация `sha512sum -c` (файл `.sha512` — hex-строка `<digest>  <filename>`,
    совместима с `sha512sum -c`; проверено research), распаковка
    `tar -xzf` → каталог `git-cliff-2.14.2/`, бинарь `git-cliff-2.14.2/git-cliff`;
  - invocation в CI без `--config` остаётся как сейчас (auto-detect
    `cliff.toml` в корне); бинарь вызывается по относительному пути
    `./git-cliff-2.14.2/git-cliff`, вне `~/.cargo/bin` (не конфликтует с
    `cache-bin` rust-cache);
  - версия pin'ится в step (2.14.2) — как сейчас в `cargo install`.
- **rust-cache** — pinned по commit SHA:
  `uses: Swatinem/rust-cache@d1527c8f67ef9af357290475d4af4b0fab8f12e0 # v2.9.3`
  (v2.9.3 — latest release; пример zeroclaw pin'ит v2.9.2), с
  `cache-on-failure: true` и `save-if: ${{ github.ref == 'refs/heads/master' }}`
  (сохранение только на push в master; PR и nightly-сборки только читают кеш).
  - step ставится после checkout/toolchain, перед cargo-шагами; в job `build`
    — после `rustup target add` (rustc-версия в ключе, target-add не меняет
    её; artifacts в `target/<triple>/release/` внутри кэшируемого `./target`).
  - дефолтные `add-job-id-key`/`add-rust-environment-hash-key` остаются
    включены (job-ключи разделяются автоматически между linux/windows/build).
- Остальные actions остаются с плавающими тегами (`checkout@v4` и т.п.) —
  pin по SHA только для нового rust-cache (минимальный diff).

## Constraints

- `ci.yml` — единственный workflow; job `release` на ubuntu-latest,
  `needs: [build]`, `if: github.event_name != 'pull_request'`.
- git-cliff — dev-tool, не crate-зависимость; `Cargo.lock` чист.
- Pin-версия git-cliff: 2.14.2 (последняя; tar.gz + `.sha512` опубликованы;
  Windows-ассеты без checksum, но они нам не нужны).
- Формат `.sha512` — hex `sha512sum`-совместимая строка (не base64, в отличие
  от нашего `release.yaml`).
- Runner'ы: ubuntu-latest (glibc), windows-latest; rust-cache работает на
  обоих (upstream CI matrix).
- Кеш: `~/.cargo` + `./target`, ключ из rustc + Cargo.lock + toolchain +
  `.cargo/config.toml` + env-vars (`CARGO_*`, `RUST*` — константы в workflow
  env, key не дрожит).

## Open questions

- none

## Changes

- 2026-11-16T11:00:58+03:00 -- v1, initial spec (draft + research): выбран gnu-бинарник, верификация `.sha512` через `sha512sum -c`, pin rust-cache по SHA v2.9.3, `save-if` master-only; PGP-верификация и правки локальных скриптов вне scope.
