# VIRShot

Ultra-lightweight gamepad-driven screenshot utility for Windows (cross-built from Linux/WSL).
Package and binary name: `virshot`.

Status: proof-of-concept stage — gamepad button presses are printed to the console and a
screenshot is captured on any press. Product requirements: `docs/spec.md`.

## Build

```bash
./cargo.sh test                                            # Linux: build + unit tests
./cargo.sh build --release --target x86_64-pc-windows-gnu # Windows: target/x86_64-pc-windows-gnu/release/virshot.exe
```

`./cargo.sh` is a wrapper that keeps cargo caches inside the repo (`./.cache/cargo`).
Native build dependencies and the full build cheat-sheet: `CONTRIBUTING.md`.
Project conventions: `AGENTS.md`.
