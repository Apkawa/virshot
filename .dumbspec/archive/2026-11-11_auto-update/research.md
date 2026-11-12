# 2026-11-11_auto-update — research notes

Superficial context scan of `/home/apkawa/code/vibe-shot` (no design decisions). Source: research subagent run 2026-11-11; facts quoted from repo files.

## 1. Dependency inventory

Root `Cargo.toml` (101 lines): workspace members `[".", "crates/overlay", "crates/toast", "crates/gamepad", "crates/icons", "crates/video-capture"]`, `exclude = ["hidpad-poc"]` (:12–14). Runtime deps (:16–70): `xcap 0.9.8 (wgc)`, `image 0.25 (png,jpeg,webp)`, `chrono 0.4.45`, `thiserror 2`, `eframe 0.37.0 default-features=false features=["glow","default_fonts","x11","wayland","links"]` (:21–25 comment: "Feature-trimmed for the <10 MB exe budget"), `serde derive`, `toml 1.1.8`, `tracing`, `tracing-subscriber env-filter`, `clap derive`, `tray-icon (ksni)`, `egui`, `egui_commonmark 0.26.0 default-features=false`, `global-hotkey 0.9.0`, `rfd 0.17.2`, path deps `overlay`/`toast`/`icons`, `rust-i18n 4.2.4`, `shadow-rs 2.0.0 default-features=false`. Target-scoped `[target."cfg(windows)".dependencies] gamepad = { path = "crates/gamepad" }` (:82–83). `[build-dependencies] shadow-rs features=["build"]` (:92–93). `[dev-dependencies] rusty-hook ^0.11.2` (:95–96). `[profile.release] opt-level="z", lto="thin", strip=true` (:98–101).

`Cargo.lock` audit (543 packages) for `self_replace|ureq|reqwest|attohttpc|curl|hyper|zip|tar|flate|sha2|sha512|base64|digest`:

- **No HTTP client at all** — no ureq/reqwest/attohttpc/curl/hyper. `webbrowser` (:4822) is only `eframe` feature `links` → `egui-winit` transitive.
- **No hashing crate** — no `sha2`/`sha512`/`digest`. `base64 0.23.1` (:367) exists only transitively via `rust-i18n-support` (:3637) ← `serde-saphyr` (:3778); not declared in any manifest.
- **No archive crate** — no `zip`/`tar`. `flate2 1.1.10` (:1348) + `crc32fast` (:794) + `fdeflate` (:1321) exist only as `image`'s png/tiff backends (:3119, :4139).
- **No `self_replace`** — absent from every manifest and from the lock.
- Crate manifests: `crates/gamepad` — zero crates.io deps (hand-written `gameinput.dll` COM shim); `crates/icons` — egui/image/thiserror; `crates/overlay` — winit/glutin/glutin-winit/glow/egui/egui-winit/egui_glow/image/thiserror; `crates/toast` — overlay path dep + egui + thiserror; `crates/video-capture` — image/thiserror/tracing + `windows-capture 2.0.1` (cfg(windows)), referenced only by its own example, not by the app.

**Constraint:** AGENTS.md "New dependencies require explicit user confirmation" — HTTP client, hash, archive, `self_replace` are all new crates.io deps and each needs approval before it is added.

## 2. Build metadata available for comparison

`build.rs` (:20–26): `shadow_rs::ShadowBuilder::builder().build_pattern(shadow_rs::BuildPattern::RealTime).build()`; :28 emits `cargo:rustc-env=VIRSHOT_BUILD_TIME` (Moscow `+03:00`, computed in `moscow_build_time()` :40–54, empty string on clock failure). `src/main.rs:84` `shadow_rs::shadow!(build);`. Constants actually read today: `build::PKG_VERSION`, `TAG`, `SHORT_COMMIT`, `BUILD_TIME_3339` (`src/about.rs:193–195,206–207,248–249,261`). **Not referenced but available:** `COMMIT_HASH`, `BUILD_TARGET`, `BUILD_RUST_CHANNEL`, `BRANCH`, `RUST_VERSION`.

Version semantics: spec §4.6 (:585–588) — version gets a `-dev` suffix when HEAD has no `v<version>` tag (`TAG` from `git tag -l --contains HEAD`, CI from `GITHUB_REF`); `about.rs::version_with_dev`.

## 3. Release artifact metadata the client must match

`scripts/package-release.sh` (110 lines): flags `--binaries --version --kind release|nightly --sha --repo --tag --out` (:30–41, `OUT` default `dist`); inputs `binaries/win-x64/virshot.exe`, `binaries/linux-x64/virshot` (:59–63); `base="virshot-$VERSION"`, nightly adds `-nightly-$SHA` (:66–69); `ZIP="$base-win-x64.zip"`, `TARBALL="$base-linux-x64.tar.gz"` (:70–71); `sha512_b64() { openssl dgst -sha512 -binary "$1" | openssl base64 -A; }` (:76–78) → **base64 of the raw 64-byte digest, 88 chars**; archives hold the bare binary only (`zip -j` :83, `tar -czf … -C linux-x64 virshot` :85); `releaseDate="$(date -u +'%Y-%m-%dT%H:%M:%S.%3NZ')"` (:87); `url_base="https://github.com/$REPO/releases/download/$TAG"` (:88); `release.yaml` (:94–107) fields: `version`, `releaseDate` (quoted ISO-8601 ms + `Z`), short `commit_sha`, `files[] {url, type, sha512, size}` with `type` = `x86_64-pc-windows-msvc` / `x86_64-unknown-linux-gnu`.

`.github/workflows/release.yml` (135 lines): triggers `push tags ['v*']`, `push branches: [master]`, `workflow_dispatch` (:4–7); `permissions: contents: write`, `GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}` (:21); build matrix `x86_64-pc-windows-msvc` → `virshot.exe`, `x86_64-unknown-linux-gnu` → `virshot`; package job (:78–110) resolves kind/tag (tag `v*` → release, else nightly), version via `sed` on Cargo.toml (:86–90), notes via `git-cliff --latest` (release) / `--unreleased` (nightly) (:94–100); publish (:111–135): assets `dist/*.zip dist/*.tar.gz dist/release.yaml`; nightly deletes every existing asset then `gh release upload` + `gh release edit "$tag" --target "$(git rev-parse HEAD)" --prerelease`; release → `gh release create "$tag" --target "$(git rev-parse HEAD)" …`. So the nightly prerelease is moved in place, and `release.yaml` is always the current asset for that tag.

`scripts/release.sh` only creates commit + annotated tag locally; ZeroVer bumps via git-cliff (`cliff.toml`, `initial_tag = "v0.1.0"`); publishing happens only in CI.

## 4. Product-spec conflict (scope guard)

- `docs/spec.md` §5 (:612–613): "никаких сетевых запросов из процесса приложения … полная приватность" — the shipped spec forbids app-side network I/O. (:600: "Само приложение не делает сетевых запросов".)
- `docs/spec.md` §10 (:858–860): "Поставка MVP: `virshot.exe` прямо из билда … **без инсталлятора и автообновления**."
- `rg` for `auto-update|self_replace|release.yaml|sha512` in `docs/spec.md`: no hits (only `OverlayHost::update` :314, About nightly note :586).
- AGENTS.md scope guard: anything not in `docs/spec.md` is out of scope until explicitly added by the user → this feature requires amending `docs/spec.md` (new section) before implementation.

## 5. Budget constraints

`docs/spec.md` §5 (:607–611): exe < 10 MB, RAM < 30 MB, CPU < 1% idle. `Cargo.toml` `[profile.release] opt-level="z", lto="thin", strip=true` (:98–101); eframe features trimmed for that budget (:21–25). Existing nightly archive is ~3.2 MB (win-x64 zip, from the draft sample) and Linux tar.gz ~5.6 MB, so the binary itself is well under budget; new deps' cost is the open part.

## 6. Settings model (General) and UI wiring

`src/config.rs` (554 lines): `pub(crate) enum Error` (:25–48) `Io { path, message, not_found }` / `Parse { path, message, line }`; `load` (:93), `load_or_default` (:140), `load_or_create` (:165), `save` (:193–197) = `toml::to_string_pretty` + `std::fs::write`; `CONFIG_FILE_NAME` (:200); config path resolution `config_path_from_env` (:219), `windows_config_candidates` (:241), `dir_writable`/`probe_dir_writable` (:257–277, probe file `virshot.probe.tmp` removed at :275); `default_config_path()` (:370–382) uses `std::env::current_exe()` (:373 — the only occurrence in the repo).

Types: `Format` (:390–413) `Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize`, serialized as lowercase `"png"/"jpg"/"webp"`, `extension()` (:404); `OverlayPosition` (:415–422) same derives, `#[default] BottomRight`, serialized with the variant name verbatim. `General` (:426–436) struct-level `#[serde(default)]`, fields `language, save_path, file_full_screen, file_window, format, jpeg_quality: u8, webp_quality: u8`; `Default for General` (:438–445) → `general_with_home(home_dir())` (:449–459). `Settings { general, controls, overlay }` (:544–550) with `Default` + `#[serde(default)]`. All `pub(crate)`; root package is bin-only.

`src/gui.rs`: tabs `General | Hotkeys | Overlay` (:629–639); `general_tab()` (:1805–1886) — language `ComboBox` + `selectable_label` (:1812–1823, `LANGUAGE_OPTIONS: [(&str,&str); 6]` :293), save path `text_edit_singleline` + Browse (:1828–1836, blocking `rfd::FileDialog` inside the pass, :1905–1919), template edits (:1840–1844), placeholder reference `CollapsingHeader` (:1851–1861), format `ComboBox` + `selectable_value` (:1863–1869), quality `DragValue` with `ui.add_enabled` (:1871–1885). **General has no checkbox today**; checkbox precedent: Overlay tab `ui.checkbox(&mut overlay.enabled, …)` (:2096), position ComboBox (:2126), `ui.checkbox(&mut overlay.on_all_monitors, …)` (:2143). Draft/applied model: `SettingsApp { config_path, applied, draft, controller, status, tab, … }` (:1278+), `is_dirty(draft, applied) = draft != applied` (:1201–1202), `BottomButtons` (:1214) / `bottom_buttons_enabled` (:1220–1223) / `BOTTOM_BAR_PANEL_ID` (:1233), `cancel_draft` (:1239), `close_decision` (:1259). `apply()` (:1490–1539): duplicate-binding gate → `config::save(&self.draft, &self.config_path)` (:1501) → `rust_i18n::set_locale` (:1509) → `tray.refresh_menu()` (:1513) → `self.applied = self.draft.clone()` (:1520) → `controller.apply(self.draft.clone())` (:1523) → publish into `settings_mailbox` (:1532–1536); save failure logs and app stays alive (:1518). Only Apply/OK writes `config.toml`; Cancel/X-close resets the draft (:2316).

## 7. i18n catalog constraint

Single `locales/app.yml`, `_version: 2`, dotted keys, locales `en/ru/zh-CN/es/ja/ko`. `src/i18n.tests.rs` pins **exactly 82 shipped keys** (:237, :356 assertions), `SUPPORTED_LOCALES: [&str; 6]` (:330), every key must have all six translations (:351) and non-notation values must differ byte-for-byte from `en` (:347), placeholder names must match in all locales (:389); catalog embedded at compile time (`I18N_CATALOG_SOURCE`, :463); locale access serialized by a static mutex (:31–39). Adding update-related strings ⇒ editing those key-count assertions.

## 8. Threading, drain point, restart contracts

- Main loop: `eframe::run_native` in `gui::run` (:2589–2693); `App::logic` (:2186–2367) is the single drain point (tray poll :2187–2235, pad capture drain :2323, probe-press drain :2331, `overlay_host.update(ctx)` :2343, hotkey retry :2352) and keeps the loop alive via `ctx.request_repaint_after(TRAY_POLL_TICK)` (:2366, `TRAY_POLL_TICK = 500ms`, :190). While the window is hidden eframe runs logic only.
- Existing background threads (all plain `std`, no async runtime anywhere): hotkey listener `src/hotkeys.rs:867` (`GlobalHotKeyEvent::receiver()`), gamepad pump `src/gamepad.rs:437–467` (resident, `stop` never set), capture `capture::spawn_capture` (:361–375, detached, **no result channel** — logs + toast chip only), toast timer `crates/toast/src/service.rs:101–113` (`mpsc::channel::<ToastCommand>`, `recv_timeout` :157–170, `Drop` sends `Quit` and joins), log writer `src/logging.rs:157–163`. Shared-state shapes: `settings_mailbox = Arc::new(Mutex::new(settings))` (`main.rs:404`), `ToastHandoff = Arc<Mutex<Option<ToastHandle>>>` (`capture.rs:184`), `debounce = Arc<Mutex<Debounce>>` (`main.rs:408`), `PadCaptureMailbox` latest-only (`gamepad.rs:114–121`). One winit `EventLoop` per process (overlay thread backend used only by examples/standalone).
- Process spawning today: only `src/instance.rs:168–178` `Command::new("tasklist")` (Windows pid liveness), `build.rs` git subprocess, `webbrowser` via eframe links, and the e2e harness (`tests/e2e/cli_headless.rs:21`). **Nothing launches or restarts `virshot.exe`.**
- Exit path: tray `Exit` is the only exit (`src/tray.rs:44–45`, `MENU_ID_EXIT` :162) → `exit_requested = true` + `ViewportCommand::Close` (:2228–2232) → `close_decision` (:1259) → `CloseAction::Exit` lets `run_native` return (:2296–2310); plain X hides to tray (:2311–2316); GUI-less park loop `park_without_gui` (`main.rs:482–505`) with `RESIDENT_SHUTDOWN`. `InstanceGuard::Drop` removes `<config>.lock` (`instance.rs:81`); `<config>.lock` + pid probe enforce one resident process per config (:147–178).
- Startup order (`main.rs:207–239`, `run_resident` :362–471): `--version` short-circuits before any file work (:209–214) → log dir → `init_logging` → `config::load_or_create` → `set_locale` → `InstanceGuard::acquire` (:381–392, exit 1 on live holder) → handoff/mailbox/debounce → hotkeys → pad pump → tray → `gui::run`. **No startup cleanup step exists today.**

## 9. Temp files / leftover cleanup today

No `std::env::temp_dir`/`%TEMP%` usage in `src/` or `crates/` (only literal `/tmp/.X11-unix` path in `hotkeys.rs:937` and test strings). Tests use repo scratch dirs under `./.tmp/` with unique tags and no tempdir dependency (`config.tests.rs:356–380`, `gui.tests.rs:47–65`, `fonts.tests.rs:9–12`). Shipped deletions: `config.rs:275` (probe removal), `instance.rs:81` (lock removal), `instance.rs:120–127` (stale lock removal). Runtime file placement: `virshot.exe`, `config.toml`, `virshot.log` (`logging.rs:17`, `default_log_path` :79–86), `config.toml.lock`, transient `virshot.probe.tmp`.

## 10. Errors, logging, UI surfacing

Per-module `thiserror` enums, all `pub(crate)`: `template.rs:50`, `bindings.rs:92` (incl. `ReservedKey`), `config.rs:25–48`, `logging.rs:39–54`, `instance.rs:20–21` (`AlreadyRunning { pid }`), `gui.rs:1269–1272` (`Error::Gui` — setup only), `tray.rs:50`, `capture.rs:69–70` (10 variants incl. `Template`, `Save`, `Io`), `hotkeys.rs:179`. Crates: `overlay/src/error.rs`, `toast/src/error.rs`, `icons/src/error.rs`, `video-capture/src/error.rs`; `crates/gamepad/src/lib.rs:95` hand-written enum (no thiserror there).
Logging: `logging::init_logging(log_path, level)` (:140–145, `OnceLock`), append-mode file writer thread (:147–191), levels `Debug | Info | Error` only (:22–26), no rotation. Surfacing: capture outcome → localized toast chip (`capture.rs:266–308` `chip_text` mapping variants to `toast.*` keys, `ToastColor::Danger` holds `ERROR_HOLD_MS`), gated by `capture::should_show` + `overlay.enabled`; GUI failures → status-row warning icon (`gui.rs:617–624`, `icons::WARNING_ICON_PNG`, decoded at :2679); config save failure → log only (:1518). Spec §5 (:614–615): any error is logged and shown as a toast, never aborts. No `unwrap()/expect()` outside tests/startup init.

## 11. Windows specifics and targets

`cfg(windows)` sites: `config.rs:204–209` (`FALLBACK_CONFIG_DIR`), :318–323 (`USERPROFILE`), :371–380 (`default_config_path` + candidates), `instance.rs:168–178` (tasklist), `gui.rs:2044` (`cfg!(windows)` pad field), `gamepad.rs` (:80–100, :437, :474, …), `hotkeys.rs:572`, `main.rs:64` `#![windows_subsystem = "windows"]`, `crates/gamepad/src/lib.rs`, `crates/overlay/src/gl_boot.rs:130`, `host.rs:354`.
Targets: shipped release archives are `x86_64-pc-windows-msvc` (win) and `x86_64-unknown-linux-gnu` (linux); AGENTS.md local build is `--target x86_64-pc-windows-gnu` (MinGW-w64); `.cargo/config.toml` lines are all commented out (:1–6); CI Windows job (`ci.yml:43–49`) tests/clippy on the host target (msvc) with no `--target`.
`#![forbid(unsafe_code)]` in root and `crates/toast`; documented exception `deny(unsafe_code)` + module-scoped `allow` only in `crates/overlay/src/gl_boot.rs`.

## 12. Test layout

Root package is bin-only ⇒ `tests/unit/main.rs` declares no modules (:1–9) ⇒ zero root integration tests; new pure logic is unit-tested via sibling `<module>.tests.rs` attached at module end (`#[cfg(test)] #[path = "x.tests.rs"] mod tests;`). Existing sibling files with counts: `config.tests.rs` 51, `gui.tests.rs` 105, `bindings.tests.rs` 46, `hotkeys.tests.rs` 42, `capture.tests.rs` 38, `gamepad.tests.rs` 38, `template.tests.rs` 33, `about.tests.rs` 18, `fonts.tests.rs` 18, `i18n.tests.rs` 15, `tray.tests.rs` 11, `instance.tests.rs` 9, `logging.tests.rs` 8 (≈432 total). `tests/e2e/cli_headless.rs` 10 tests driving the built binary headless. Gates: `./scripts/check.sh` (check+test+clippy `-D warnings`, `SKIP_E2E=1` filters `cli_headless::`), `./scripts/check_full.sh` before commit; Windows gate `./cargo.sh test --no-run --target x86_64-pc-windows-gnu`; `./cargo.sh` pins `CARGO_HOME=./.cache/cargo`. Injected time at module boundaries (`capture.rs:372` `now`, `template::render(..., now, ...)`, `instance.rs:155–157` `pid_alive(pid, probe)`).

## Facts that constrain the feature

1. Four capabilities absent from manifests and lock: HTTP client, sha512 hashing, archive unpack, `self_replace` → each is a new crates.io dependency requiring explicit user approval (AGENTS.md).
2. `docs/spec.md` §5 forbids app-side network requests and §10 explicitly excludes auto-update → the product spec must be amended (new section) before implementation; scope guard applies.
3. Size budget: exe < 10 MB with `opt-level="z"`, `lto="thin"`, `strip=true`; new deps must stay cheap (feature-trimmed, like eframe/egui_commonmark precedent).
4. Root bin-only, all types `pub(crate)` → sibling `.tests.rs` only; no `tests/unit/` import possible.
5. `General` needs `Eq`/`Serialize`/`Deserialize` + `#[serde(default)]` per-field defaults for new update settings; enum serialization precedents: lowercase strings (`Format`), verbatim variant names (`OverlayPosition`).
6. New UI strings must exist in `locales/app.yml` for all six locales; `i18n.tests.rs` pins exactly 82 keys — assertions must be updated.
7. No async runtime; background work = plain `std` thread + `mpsc`, drained in the eframe logic pass (500 ms repaint tick keeps it alive even when the window is hidden).
8. Restart/replacement collides with resident-process contracts: tray `Exit` is the only exit, `<config>.lock` + pid probe enforce a single resident process, pad pump thread is never stopped, toast/log threads join or detach. A self-replace + relaunch must release the lock and terminate the old process cleanly.
9. No temp-dir facility today; repo convention is `./.tmp/` scratch for tests and sibling-of-config placement for runtime files. `%TEMP%` would be new ground.
10. `release.yaml` contract: `version`, `releaseDate`, short `commit_sha`, `files[] {url, type, sha512, size}`, `sha512` = base64 of raw digest (88 chars), `type` = Rust target triple, archive name per platform; nightly tag is prerelease and moves in place.
11. Comparison inputs available from build metadata: `PKG_VERSION`, `SHORT_COMMIT`, `COMMIT_HASH`, `BUILD_TARGET` (currently unused but present).
12. `-dev` suffix rule (spec §4.6) means a locally built binary reports `0.1.0-dev` — version comparison against `release.yaml` `version` must tolerate that.
13. Target mismatch: shipped Windows binary is `x86_64-pc-windows-msvc`; local dev builds are `-gnu`. Choosing `files[].type` by `BUILD_TARGET`/`std::env::consts` must map correctly.
14. Release notes already exist as the GitHub release body (git-cliff `--latest` / `--unreleased`), which the GitHub API exposes as `body`; locally there is `CHANGELOG.md` via `include_str!` in About.
15. No credentials today; the download URL pattern is unauthenticated and public; GitHub API anonymous rate limit applies to release listing.
16. `docs/spec.md` §10 test-count note (:839–857, 392 unit + 9 e2e) is stale versus current counts (≈432 unit + 10 e2e).

## Stage 1 — dependency verification (subagent 80ce10d6, 2026-11-11)

Read-only verification + scratch harness under `./.tmp/depcheck/` (own `[workspace]`, own lock/target, repo-local `CARGO_HOME`). Repo manifests/lock untouched (md5 verified before/after). Toolchain rustc/cargo 1.99.1, edition 2024.

### Approved dependency block (verified by `./cargo.sh add --dry-run`, zero version bumps of existing entries)

```toml
ureq = { version = "3.5.1", default-features = false, features = ["rustls"] }
sha2 = { version = "0.11.1", default-features = false }
zip = { version = "9.5.1", default-features = false, features = ["deflate-flate2"] }
flate2 = { version = "1.1.10", default-features = false, features = ["rust_backend"] }
serde_json = "1.0.154"
self-replace = "1.5.1"
```

All six compile and link for `x86_64-pc-windows-gnu` (ring's C builds via `x86_64-w64-mingw32-gcc`, windows-sys imports resolve) and for the linux host; runtime smoke tests on linux passed (sha512 chunk-stream == openssl base64, zip deflate entry read byte-exact, flate2 `GzDecoder` gunzip exact, ureq redirect GET → 200, `serde_json` parse). `#![forbid(unsafe_code)]` in the root compiles fine while calling `self_replace` (unsafe is third-party only).

### Per-crate facts

- **self-replace 1.5.1** (package name `self-replace`; `self_replace = "1.5.1"` resolves identically). API: `pub fn self_replace<P: AsRef<Path>>(new_executable: P) -> Result<(), io::Error>`; also `self_delete()`, `self_delete_at`, `self_delete_outside_path`. **No** `directory_of_current_exe` / `self_replace_exec` in 1.5.1 — use `std::env::current_exe()` ourselves. Windows mechanics (`src/windows.rs`): renames the running exe to `<exe dir>/virshot.__relocated__.exe`, schedules deletion via an AtExit hook (`.CRT$XCV`) in a spawned copy `<exe dir or temp_dir>/virshot.<32 lowercase fastrand chars>.__selfdelete__.exe`, then copies the new binary to `<exe dir>/virshot.<rand>.__temp__.exe` and renames over the original. Unix (Linux/macOS): symlink-resolving, copies the new binary to `<exe dir>/.<stem>.__temp__*` then renames over. Other platforms → `unimplemented!()`.
  **Startup leftover GC must delete:** `virshot*.__relocated__.exe` and `virshot*.__temp__.exe` in the exe directory, `virshot*.__selfdelete__.exe` in `std::env::temp_dir()`, plus our own download archive/subdir. Note the pattern differs per platform (`.<stem>.__temp__*` on unix).
  Its deps are already locked: tempfile 3.27.0, fastrand 2.5.0, windows-sys 0.61.2, getrandom 0.4.3, once_cell 1.21.4, rustix 1.1.6.
- **ureq 3.5.1** — blocking, no tokio/hyper. Config API: `ureq::Agent::config_builder()` → `.timeout_global(Some(Duration))` (+ `timeout_per_call`/`timeout_connect`/`timeout_recv_body`), `.user_agent(..)`, `.max_redirects(10)`, `.save_redirect_history(true)`, `.http_status_as_error(false)`, `.build()` → `ureq::Agent::new_with_config(config)`. `agent.get(url).call()?`; `res.status()` is `http::StatusCode` (`.into()` for u16); `res.body_mut().read_to_string()?` / `read_to_vec`. There is **no** `res.uri()`; final URL only via `ResponseExt::get_redirect_history()` (requires `save_redirect_history(true)`). Redirects follow by default (`max_redirects = 10`, error when exceeded). Minimal TLS: `default-features = false, features = ["rustls"]` (`default = ["rustls","gzip"]`; `rustls = ["rustls-no-provider","_ring","rustls-webpki-roots"]`; gzip → flate2 stays OFF). rustls 0.23.46 + ring 0.17.14 (C via cc 1.10.0, already locked) + bundled webpki-roots (no OS cert store — fine on windows-gnu/msvc).
  New lock entries: ureq, ureq-proto 0.7.1, utf8-zero 0.8.1, `http` 1.5.0, rustls, rustls-pki-types, rustls-webpki, ring, untrusted, webpki-roots, subtle, zeroize(+zeroize_derive), getrandom 0.2.17. base64 0.23.1 (exact locked version), percent-encoding, log, memchr already locked.
- **sha2 0.11.1** — pure Rust (cpufeatures 0.3.1 locked). Config: `default-features = false` (drops `oid`/`alloc`). **API correction vs the draft plan:** digest 0.11 does **not** implement `io::Read` streaming (`update(&mut reader)` existed in digest 0.10 only; separate `digest-io` crate). Streaming = chunk loop: `let mut h = Sha512::new(); loop { read 64 KiB chunks; h.update(&buf[..n]); }` → `h.finalize()`, `.as_slice()` → raw 64 bytes. Byte-exact match against `openssl dgst -sha512 -binary | openssl base64 -A` verified empirically. New lock entries: sha2, digest 0.11.3, block-buffer 0.12.1, crypto-common 0.2.2, hybrid-array 0.4.15, typenum 1.20.1 (typenum absent today).
- **zip 9.5.1** — read path: `zip::ZipArchive::new(File)` → `archive.by_name("virshot.exe")` → `ZipFile: Read` → `io::copy`. Minimal features: `default-features = false, features = ["deflate-flate2"]` (`deflate-flate2 = ["_deflate-any","dep:flate2"]`); default features would pull aes/bzip2/deflate64/lzma/ppmd/zstd/xz/time (C or heavy). **Consumer must declare `flate2` itself with `rust_backend`** so feature unification enables miniz_oxide for zip's deflate copy (empirically proven). No `deflate-raw` feature in 9.5.1. New lock entries: zip, constant_time_eq 0.6.1, typed-path 0.13.0.
- **flate2** — locked 1.1.10 (transitive via `image`). Declaring `1.1.10` keeps the locked version (cargo never bumps a satisfying locked entry). Feature name proof: `./cargo.sh add --dry-run flate2@1.1.10 --no-default-features --features rust` → `error: unrecognized feature ... crate flate2`; the old `"rust"` feature is gone — use **`rust_backend`** (`["miniz_oxide","any_impl"]`). Zero churn, only the root-package edge + feature flags. `GzDecoder` verified against a gzip file.
- **serde_json 1.0.154** — already locked (transitive via rust-i18n-macro/rust-i18n-support), exact match to latest release. Declaring it adds only a root-package edge; itoa/memchr/serde_core/zmij locked. Default features fine; `serde_json::from_str::<Value>` verified.
- **base64 0.23.1** — transitively locked, zero usage in `src/`/`crates/` (`rg 'base64|b64'`). Hand-rolled ~25-line decoder stands (verified byte-exact against openssl base64 output, padding/newline tolerant). Declaring the crate would add no new lock package but is still a manifest change; recommendation: keep the hand-rolled decoder.
- **Version floors:** self-replace 1.63, flate2 1.67, ureq 1.85, sha2 1.88, zip 1.88 — all ≤ rustc 1.99.1, all compatible with edition 2024 consumers.

### Size budget finding (BLOCKING)

Release profile `opt-level="z", lto="thin", strip=true`, dep code kept reachable via `black_box` so the linker does not dead-strip:
- std-only baseline: 264 KB (windows-gnu) / 312 KB (linux)
- zip + sha2 + flate2 + serde_json + self-replace: ≈ +229 KB (win) / +238 KB (linux)
- ureq + rustls + ring: ≈ +1.46 MB (win) / +1.87 MB (linux)
- Full harness exe: 2,025,984 B (win-gnu) / 2,415,592 B (linux)
- Current shipped windows-gnu `virshot.exe`: **8,992,256 B** → projected ≈ **10.5–10.7 MB**, i.e. likely just over the `docs/spec.md` §5 budget (exe < 10 MB).
- Trim options measured: hand-parsing a single-entry zip over already-linked flate2 saves only ≈ 0.15 MB — not enough. rustls+ring ≈ 1.46 MB is the irreducible TLS cost (blocking HTTP over HTTPS, no async, no native OpenSSL).

### Other operational facts

- GitHub API `releases/latest` JSON fetch works (correct UA required), but the sandbox IP is currently rate-limited (403 rate-limit JSON via both ureq and curl) → configure `http_status_as_error(false)` and treat non-2xx as data; lazy fetch only when the dialog opens keeps usage ≤ 60/h/IP.
- `https://github.com/Apkawa/virshot/releases/latest/download/release.yaml` currently returns **404**: the repo has no stable (non-prerelease) release yet, only the `nightly` prerelease. Nightly URL works today (200 after redirect to `release-assets.githubusercontent.com`; real `release.yaml` fields confirmed: base64-of-raw-digest `sha512`, `size`, `url`, `type`).
- Windows `self_replace` runtime behavior cannot be executed on this Linux host — compiled+linked only; needs a Windows-host acceptance run.
- No registry blockers: cargo resolved and downloaded everything into `./.cache/cargo`.
- Verified directly (curl, 2026-11-11T15:47+03:00): nightly `release.yaml` fields are `version: 0.1.0`, quoted `releaseDate: '2026-11-10T11:29:09.928Z'`, `commit_sha: f710773`, two `files[]` entries (`x86_64-pc-windows-msvc` zip 3,244,195 B; `x86_64-unknown-linux-gnu` tar.gz 5,617,967 B). Both `sha512` values are **88 characters** (base64 of the raw 64-byte digest with `==` padding), unquoted TOML strings — the subagent's "44-char" note was a misread; the hand-rolled decoder must accept 88 chars (64 raw bytes) and tolerate trailing padding/newlines.

### Release size measurement (Stage 1 acceptance, 2026-11-11)

Verification build on the linux host after the approved dependency block was added to root `Cargo.toml` (uncommitted). rustc/cargo 1.99.1, `./cargo.sh` wrapper. Only this file was modified; no commit, no `cargo fmt`.

| Measurement | Result |
|---|---|
| `./cargo.sh build --release` (linux, x86_64-unknown-linux-gnu) | exit 0 → `target/release/virshot` = **15,799,864 B** (~15.1 MiB, ELF PIE, stripped) |
| `./cargo.sh build --release --target x86_64-pc-windows-gnu` | exit 0 → `target/x86_64-pc-windows-gnu/release/virshot.exe` = **9,000,448 B** (~8.6 MiB, PE32+ GUI, stripped). First REAL release link including ring/rustls via MinGW — no linker errors |
| Delta vs pre-change baseline 8,992,256 B | **+8,192 B** (+0.09%) |
| Verdict vs revised budget **exe < 12 MB** | **HOLDS**: 9,000,448 B < 12 MB (comfortable headroom vs both 12,582,912 B MiB and 12,000,000 B decimal readings) |

- Build profile: `opt-level="z", lto="thin", strip=true` (`[profile.release]`); CI ships `x86_64-pc-windows-msvc`, whose size may differ from this local `-gnu` measurement.
- Stage 1 adds the six deps with no code referencing them yet, so thin-LTO/linker dead-code elimination strips most of the new dep code (hence the +8 KB delta). When Stages 2–3 wire ureq/rustls/ring + zip/sha2/flate2/serde_json/self-replace into the update modules, the projected final size ≈ 10.5–10.7 MB (research projection above) — still under the revised 12 MB budget.
- The linux host binary (15.8 MB) is not covered by the spec §5 budget: §5 targets the shipped Windows `virshot.exe`; linux is the dev-host build (wayland/pipewire/GL stack).
- RAM/CPU expectations (RAM < 30 MB, CPU < 1% idle) are NOT measurable here: no GUI/Windows runs attempted on this Linux host — size-only acceptance.

## Stage 3 amendment — `release.yaml` is YAML, not TOML (2026-11-11)

Finding during Stage 3: the shipped `release.yaml` asset uses YAML colon syntax
(`version: 0.1.0`, `releaseDate: '...'`, `- url:`), verified byte-for-byte against
the live nightly asset and `scripts/package-release.sh:94-107`. Strict `toml` 1.1.8
rejects it (`key with no value, expected '='` — probe in `./.tmp/toml-probe`), so the
spec's "parse with existing toml" assumption was wrong.

Options considered: (A) hand-rolled YAML-subset parser (~150 lines, zero manifest
change), (B) declare the already-locked `serde-saphyr 1.3.0`, (C) change the CI
generator to emit TOML (the already-published nightly asset would stay YAML ⇒ two
parse paths). User chose **B** (2026-11-11): `Cargo.toml` gains one declaration line,
`Cargo.lock` gains exactly one dependency-edge line for the `virshot` package
(numstat 1 added / 0 removed), **zero new packages** — the crate already sits in the
lock transitively via `rust-i18n` → `rust-i18n-macro` → `rust-i18n-support`
(`codegen` feature enables `dep:serde-saphyr` with default features, so feature
unification leaves the graph unchanged). `parse_manifest` is now
`serde_saphyr::from_str::<ReleaseManifest>(text)` mapped to `UpdateError::Parse`.

Size impact (measured, release builds, `opt-level="z", lto="thin", strip=true`):

| target | before amendment | after amendment | delta |
| --- | --- | --- | --- |
| linux dev-host `target/release/virshot` | 15,799,864 B | **15,813,944 B** | +14,080 B |
| windows-gnu `target/x86_64-pc-windows-gnu/release/virshot.exe` | 9,000,448 B | **9,022,464 B** | +22,016 B |

serde-saphyr previously existed only in the compiler-side proc-macro graph, so its
runtime objects were never linked; referencing `from_str` pulls the deserializer in.
Windows exe stays far under the revised **exe < 12 MB** budget.

Behavior notes accepted with the switch: duplicate keys still rejected
(`DuplicateKeyPolicy::Error` default); unknown keys ignored (deliberately **no**
`deny_unknown_fields` — the asset may gain fields later, strictness is about required
fields); a present-but-empty `files: []` now parses Ok (empty Vec) and the later
`select_file` yields `NoFileForTarget` instead of a parse error. Error messages carry
serde-saphyr's `line X column Y` wording, so the two parse-error tests assert on
substrings rather than exact text.
