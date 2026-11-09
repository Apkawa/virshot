## [unreleased]

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
- *(release)* Add cliff.toml with ZeroVer bump config
- *(release)* Add scripts/release.sh (changelog + ZeroVer bump/tag)

### 🐛 Bug Fixes

- Windows_subsustem define
- Game window screenshot, xcap[wgc]

### 📚 Documentation

- Crate-doc reader script and API-knowledge rule
- Move githooks note into the formatting & pre-commit section
- Document one-time git hooks setup in CONTRIBUTING
- Single source of truth for the gate — hooks reuse check.sh, docs stop duplicating the chain

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
- *(dumbspec)* Add git-cliff release task (draft, research, spec v1, plan)
- *(dev)* Pin git-cliff 2.14.2 dev tool (install + AGENTS.md note)
