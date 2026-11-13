## [0.1.11] - 2026-11-13

### ⚙️ Miscellaneous Tasks

- Merge release to one ci file
## [0.1.10] - 2026-11-13

### ⚙️ Miscellaneous Tasks

- Try fix release wf  v8
- *(release)* V0.1.10
## [0.1.9] - 2026-11-13

### ⚙️ Miscellaneous Tasks

- Try fix release wf  v7
- *(release)* V0.1.9
## [0.1.8] - 2026-11-13

### ⚙️ Miscellaneous Tasks

- Try fix release wf  v6
- *(release)* V0.1.8
## [0.1.7] - 2026-11-13

### ⚙️ Miscellaneous Tasks

- Try fix release wf  v5
- *(release)* V0.1.7
## [0.1.6] - 2026-11-13

### ⚙️ Miscellaneous Tasks

- Try fix release wf typo v4
- *(release)* V0.1.6
## [0.1.5] - 2026-11-13

### ⚙️ Miscellaneous Tasks

- Try fix release wf v4
- *(release)* V0.1.5
## [0.1.4] - 2026-11-13

### ⚙️ Miscellaneous Tasks

- Try fix release wf v3
- *(release)* V0.1.4
## [0.1.3] - 2026-11-13

### ⚙️ Miscellaneous Tasks

- Try fix release wf v2
- *(release)* V0.1.3
## [0.1.2] - 2026-11-13

### ⚙️ Miscellaneous Tasks

- Try fix release wf
- *(release)* V0.1.2
## [0.1.1] - 2026-11-13

### ⚙️ Miscellaneous Tasks

- Change release message
- Fix ci wf tags
- *(release)* V0.1.1
## [0.1.0] - 2026-11-13

### 🚀 Features

- *(poc)* Gamepad-driven screenshot proof of concept
- *(poc)* Gameinput share-button poc proving share/guide from a console process
- *(virshot)* Mvp screenshot utility with hotkeys, tray and gui
- *(qa)* Add ui-verify and prof scripts, move unit tests to sibling files
- *(gamepad)* Windows gamepad trigger via crates/gamepad library
- *(overlay,toast,virshot)* Overlay chip windows and toast notifications
- *(icons)* Icon assets crate with app and warning icons
- *(config)* Home-resolved save dir Pictures/ViRShot, shorter filename templates
- *(gui)* Settings window layout, wider key set, Windows bind probe
- *(i18n)* Localize the UI in six languages
- *(video-capture)* Isolated screen recording crate over windows-capture
- *(release)* Add git-cliff release helper with zerover bump/tag
- *(ci)* Add github actions test and release pipelines
- *(about)* Add About dialog with build metadata and embedded changelog
- *(update)* Add automatic updates with release checks, installer, and self-replace
- *(autostart)* Windows autostart checkbox with HKCU Run sync
- *(icon)* Redraw icon and inject ico to exe

### 🐛 Bug Fixes

- Windows_subsustem define
- Game window screenshot, xcap[wgc]
- *(ci)* Make workspace tests pass on github runners
- *(ci)* Pass gh release target commit via --target
- *(update)* Suppress the date trigger when the release commit sha matches the build
- *(autostart)* Create the HKCU Run key when absent
- *(tray)* Reorder tray menu

### 📚 Documentation

- Crate-doc reader script and API-knowledge rule
- Move githooks note into the formatting & pre-commit section
- Document one-time git hooks setup in CONTRIBUTING
- Single source of truth for the gate — hooks reuse check.sh, docs stop duplicating the chain
- Update readme, add en, zh translations

### ⚡ Performance

- *(gui)* Repaint overlays and dialogs on a fixed interval

### 🚜 Refactor

- *(gui)* Split gui.rs into functional modules and move update modules into src/update/

### 🧪 Testing

- *(update)* Fix assert update info against the current platform's manifest entry

### ⚙️ Miscellaneous Tasks

- Initial
- Grill spec
- Conventions, toolchain pin, build wrapper and MVP deps
- Rename project to VIRShot and add README
- Move rusty-hook git hooks to tracked .githooks wired to repo-local cargo cache
- Add check.sh and check_full.sh gates, document when to run them
- Update .githooks
- Remove .githooks
- Up backlog
- Update test rules AGENTS.md
- *(dumbspec)* Archive settings-window-fixes task artifacts
- Export github token for gh in release workflow
- Windows skip e2e
- Windows skip e2e v2 (oops)
- Release wf after success CI
- Fix wf release conditions
- Fix scripts/release.sh
- *(release)* V0.1.0
