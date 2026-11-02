# hidpad-poc

Proof-of-concept: receive **Share** and **Guide** button events from a plain
console process on Windows, with no window and no foreground focus. It does this
by setting GameInput's background focus policy (`0x1C0` =
EnableBackgroundInput | EnableBackgroundGuideButton | EnableBackgroundShare)
*before* registering callbacks, then pumping callbacks on its own thread through
an `IGameInputDispatcher`.

The GameInput COM interface is reached directly through `gameinput.dll` exports
(`GameInputCreate`, `GameInputInitialize`) with hand-written vtable mirrors
transcribed from `vendor/GameInput.h`; there are no crate dependencies.

## Commands

```sh
cargo test                                   # layout offset tests + build-stamp tests (Linux host)
cargo build --offline --target x86_64-pc-windows-gnu   # cross-build (MinGW-w64)
```

## Run mode

```sh
hidpad-poc.exe            # same as --gameinput
hidpad-poc.exe --gameinput
```

The PoC has exactly one mode. It prints the build stamp, the negotiated
interface version, the focus policy it set, one device line per enumerated
controller (`vid`/`pid`, `supportedSystemButtons=0x3` for an Xbox Series pad),
controller button bit changes, `share`/`guide` DOWN/UP edges, and an `alive:`
line every 5 s. Windows-only: run it on a Windows host with a connected Xbox
controller.

## Live-run caveats

- Disable Xbox Game Recording / Broadcasting: they claim the Share button before
  this process can see it.
- Xbox Game Bar can intercept Guide; expect missing Guide edges while it is
  active.
- LT/RT are analog axes (`GetControllerAxisState`), not controller buttons, so
  they are out of scope here.

## Next step

The Share edge is the future hook point: `virshot src/main.rs:72`
(`on_button_pressed`) is the integration target for a GameInput-backed trigger.
