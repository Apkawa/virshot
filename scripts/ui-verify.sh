#!/usr/bin/env bash
# ui-verify.sh — manual UI verification for the resident `virshot` app.
#
# One invocation = one Xvfb display + one private D-Bus session + one resident
# app. Everything (config, log, captures, tray evidence) lands in
# ./.tmp/ui-verify/<ts>/, and nothing survives the run.
#
# Subcommands:
#   start  start the app, assert the settings window exists (+ geometry)
#   window window geometry, window PNG (non-blank), root shot
#   shot   hotkey capture: windowfocus + `xdotool key Print` -> Saved: + file
#   tray   D-Bus tray evidence: bus name, ToolTip, IconPixmap PNG (app asset),
#          menu labels, Activate/Settings re-show, menu Exit terminates the app
#   log    log-content assertions (hotkey registration + tray ready, no-WM)
#   stop   tray menu Exit (dbusmenu Event) -> clean exit + lock removed
#   all    window + log + shot + tray + wm extras + stop
#
# Options:
#   --config <path>  use this config instead of the generated scratch config
#   --wm             run openbox: EWMH (incl. window `_NET_WM_ICON`) +
#                  `shot --window` + window hide/re-show; global hotkey
#                  registration FAILS under a WM, so hotkey asserts are skipped
#   --keep           keep the scratch dir (default: delete it on exit)
#   --timeout <s>    poll budget for window/exit waits (default 20)
#
# Examples:
#   ./scripts/ui-verify.sh all              # no WM: hotkeys + tray via D-Bus
#   ./scripts/ui-verify.sh all --wm --keep  # EWMH extras, artifacts kept
#   ./scripts/ui-verify.sh tray --keep      # tray evidence only
#   ./scripts/ui-verify.sh shot --config ./config.toml
#
# Tray evidence is D-Bus, not pixels: there is no StatusNotifierWatcher on this
# host and tint2 cannot render SNI items. dbusmenu Event uses event key
# "clicked" — the app's ksni backend ignores any other key ("left" included).
set -euo pipefail

BIN=./target/debug/virshot
SCRATCH_ROOT=./.tmp/ui-verify

# --- inner mode: runs inside Xvfb + dbus-run-session -------------------------
if [ "${UIVERIFY_INNER:-}" = 1 ]; then
  D="$UIVERIFY_DIR"
  CFG="$UIVERIFY_CONFIG"
  LOG="$UIVERIFY_LOG"
  WM="$UIVERIFY_WM"
  TIMEOUT="$UIVERIFY_TIMEOUT"
  MODE="$UIVERIFY_MODE"
  SAVE_DIR="$UIVERIFY_SAVE_DIR"
  APP=""
  OB=""
  WIN=""
  FAILS=0

  pass() { printf 'PASS %s\n' "$1"; }
  fail() { printf 'FAIL %s\n' "$1"; FAILS=$((FAILS + 1)); }
  skip() { printf 'SKIP %s\n' "$1"; }
  note() { printf '     %s\n' "$1"; }

  cleanup_inner() {
    if [ -n "$APP" ] && kill -0 "$APP" 2>/dev/null; then kill -TERM "$APP" 2>/dev/null || true; fi
    if [ -n "$OB" ] && kill -0 "$OB" 2>/dev/null; then kill -TERM "$OB" 2>/dev/null || true; fi
    rm -f "$CFG.lock"
  }
  trap cleanup_inner EXIT INT TERM

  find_window() {
    for _ in $(seq 0 $((TIMEOUT * 10))); do
      WIN=$(xdotool search --name virshot 2>/dev/null | head -1 || true)
      [ -n "$WIN" ] && return 0
      sleep 0.1
    done
    return 1
  }

  map_state() { xwininfo -id "$WIN" 2>/dev/null | grep -m1 'Map State:' || true; }

  wait_map_state() { # $1 = IsViewable | IsUnMapped
    for _ in $(seq 0 $((TIMEOUT * 10))); do
      case "$(map_state)" in *"$1"*) return 0 ;; esac
      sleep 0.1
    done
    return 1
  }

  geometry() { xwininfo -id "$WIN" 2>/dev/null | awk '/Width:/ {w=$2} /Height:/ {h=$2} END {print w, h}'; }

  unique_colors() { # count of distinct colors in a PNG
    convert "$1" -depth 8 -format '%c' histogram:info:- 2>/dev/null | wc -l
  }

  start_app() {
    if [ "$WM" = 1 ]; then
      openbox >/dev/null 2>&1 & OB=$!
      sleep 1.2
    fi
    env -u WAYLAND_DISPLAY "$BIN" --config "$CFG" --log "$LOG" >"$D/stdout.txt" 2>&1 & APP=$!
    printf '%s\n' "$APP" >"$D/app.pid"
    if [ -n "$OB" ]; then printf '%s\n' "$OB" >"$D/openbox.pid"; fi
    find_window || { fail "settings window not found within ${TIMEOUT}s"; return 1; }
    sleep 0.5
  }

  assert_window() {
    local geom W H
    geom=$(geometry)
    W=${geom%% *}; H=${geom##* }
    note "window=$WIN geometry=$W x $H $(map_state)"
    if [ -n "$W" ] && [ -n "$H" ] && [ "$W" -ge 700 ] && [ "$W" -le 740 ] && [ "$H" -ge 500 ] && [ "$H" -le 540 ]; then
      pass "window found + geometry ~720x520 ($W x $H)"
    else
      fail "window geometry not ~720x520 ($W x $H)"
    fi
  }

  assert_window_visual() {
    assert_window
    wait_map_state IsViewable && pass "window mapped (Map State: IsViewable)" || fail "window never mapped"
    if timeout 20 import -window "$WIN" "$D/shots/window.png" >"$D/shots/import.err" 2>&1; then
      local n
      n=$(unique_colors "$D/shots/window.png")
      note "$(identify "$D/shots/window.png" 2>&1) unique_colors=$n"
      [ "$n" -gt 100 ] && pass "window shot non-blank ($n unique colors)" || fail "window shot blank ($n unique colors)"
    else
      fail "import -window failed: $(head -c 120 "$D/shots/import.err")"
    fi
    if timeout 20 scrot -o "$D/shots/root.png" >"$D/shots/scrot.err" 2>&1; then
      note "$(identify "$D/shots/root.png" 2>&1)"
      pass "root shot taken ($(identify -format '%wx%h' "$D/shots/root.png"))"
    else
      fail "root shot failed: $(head -c 120 "$D/shots/scrot.err")"
    fi
  }

  assert_log() {
    if [ "$WM" = 1 ]; then
      skip "hotkey assertions (global-hotkey registration fails with a WM running)"
      note "$(grep -m2 'Hotkey not registered' "$LOG" || true)"
    else
      grep -q 'Hotkey registered: PrintScreen (full_screen)' "$LOG" \
        && pass "log: Hotkey registered: PrintScreen (full_screen)" || fail "log: Hotkey registered: PrintScreen (full_screen) missing"
      grep -q 'Hotkey registered: alt+PrintScreen (window)' "$LOG" \
        && pass "log: Hotkey registered: alt+PrintScreen (window)" || fail "log: Hotkey registered: alt+PrintScreen (window) missing"
      grep -q 'Hotkey reserved (rect area, out of MVP scope)' "$LOG" \
        && pass "log: Hotkey reserved (rect area, out of MVP scope)" || fail "log: reserved rect line missing"
    fi
    grep -q 'System tray ready' "$LOG" \
      && pass "log: System tray ready" || fail "log: System tray ready missing"
    grep -q 'Instance lock acquired' "$LOG" \
      && pass "log: Instance lock acquired" || fail "log: Instance lock acquired missing"
  }

  assert_hotkey_capture() {
    if [ "$WM" = 1 ]; then
      skip "hotkey capture (hotkeys are not registered under a WM)"
      return 0
    fi
    local saved="" attempt
    for attempt in 1 2 3; do
      timeout 10 xdotool windowfocus --sync "$WIN" >/dev/null 2>&1 || true
      timeout 10 xdotool key Print >/dev/null 2>&1 || true
      for _ in $(seq 0 $((TIMEOUT * 2))); do
        saved=$(grep -m1 'Saved: ' "$LOG" || true)
        [ -n "$saved" ] && break
        sleep 0.5
      done
      [ -n "$saved" ] && break
    done
    if [ -n "$saved" ]; then
      local path=${saved#*Saved: }
      note "$saved"
      [ -f "$path" ] && pass "hotkey capture produced $path" || fail "hotkey capture log line but no file at $path"
      case "$path" in "$SAVE_DIR"*) pass "capture landed in save_path ($SAVE_DIR)" ;; *) fail "capture outside save_path ($SAVE_DIR)" ;; esac
      timeout 20 identify "$path" >"$D/shots/saved.identify" 2>&1 && pass "capture file valid image: $(cat "$D/shots/saved.identify")" || fail "capture file is not a readable image"
    else
      fail "no 'Saved:' log line after 3 hotkey attempts"
      note "$(grep -m2 'Capture failed\|foreground window' "$LOG" || true)"
    fi
    # Toast evidence (Stage 4, spec §4.3): the chip is no longer drawn inside
    # the settings window — crates/toast hands it to crates/overlay's OverlayHost,
    # which spawns it as a CHILD egui viewport window of this pid. The chip
    # lives `duration_ms` (~2 s), so the window and the corner pixels are
    # asserted right after the 'Saved:' line, not from a log line.
    grep -m1 'Toast service started on' "$LOG" >/dev/null 2>&1 \
      && { note "$(grep -m1 'Toast service started on' "$LOG")"; pass "log: toast service started"; } \
      || fail "log: toast service start line missing"
    local chips=""
    for _ in $(seq 0 10); do
      chips=$(xdotool search --pid "$APP" 2>/dev/null | wc -l | tr -d ' ' || true)
      [ "${chips:-0}" -ge 2 ] && break
      sleep 0.3
    done
    if [ "${chips:-0}" -ge 2 ]; then
      pass "toast chip window present (app has $chips window(s): settings + chip)"
    else
      fail "no toast chip window (app has ${chips:-0} window(s))"
    fi
    if timeout 20 import -window root -crop 260x70+1015+915 +repage "$D/shots/toast-corner.png" 2>>"$D/shots/import.err"; then
      local n
      n=$(unique_colors "$D/shots/toast-corner.png")
      note "$(identify "$D/shots/toast-corner.png" 2>&1) unique_colors=$n"
      [ "$n" -gt 3 ] && pass "toast corner non-blank ($n unique colors)" || fail "toast corner blank ($n unique colors)"
    else
      fail "toast corner crop failed: $(head -c 120 "$D/shots/import.err")"
    fi
    # crates/toast reports every show/spawn failure on stderr (merged into
    # stdout.txt); a clean run prints nothing from it.
    if grep -q '^toast: ' "$D/stdout.txt" 2>/dev/null; then
      fail "toast crate reported a failure: $(grep -m1 '^toast: ' "$D/stdout.txt")"
    else
      pass "toast crate reported no failure"
    fi
  }

  tray_name() {
    timeout 10 dbus-send --print-reply --dest=org.freedesktop.DBus /org/freedesktop/DBus \
      org.freedesktop.DBus.ListNames 2>/dev/null | grep -o 'org.kde.StatusNotifierItem-[0-9]*-[0-9]*' | head -1 || true
  }

  read_layout() { # $1 = tray bus name; writes tray/layout.txt
    timeout 20 python3 "$D/tray/sni.py" "$1" layout >"$D/tray/layout.txt" 2>&1
  }

  item_id() { # $1 = label; reads tray/layout.txt
    awk -v label="$1" '$1 == "ITEM" && $3 == label {print $2}' "$D/tray/layout.txt" 2>/dev/null | head -1 || true
  }

  assert_tray() {
    local NAME id_settings id_exit
    NAME=$(tray_name)
    if [ -n "$NAME" ]; then pass "tray bus name present ($NAME)"; else fail "tray bus name missing"; return 1; fi

    if timeout 20 python3 "$D/tray/sni.py" "$NAME" props "$D/tray/icon.raw" "$D/tray/icon.png" >"$D/tray/props.txt" 2>&1; then
      note "$(cat "$D/tray/props.txt")"
      grep -q 'virshot' "$D/tray/props.txt" && pass "ToolTip carries virshot" || fail "ToolTip missing virshot text"
      # Tray icon = embedded app asset (icons::APP_ICON_PNG, 128×128 RGBA PNG),
      # not the old procedural 32×32 glyph: 128*128*4 = 65536 ARGB bytes.
      grep -q 'IconPixmap 128x128 bytes=65536' "$D/tray/props.txt" \
        && pass "IconPixmap is 128x128 app asset (bytes=65536)" || fail "IconPixmap is not 128x128 bytes=65536"
      if timeout 20 identify -format '%wx%h\n' "$D/tray/icon.png" 2>/dev/null | grep -q '128x128'; then
        pass "IconPixmap converted to PNG 128x128 ($D/tray/icon.png)"
        # Real camera asset, not transparent garbage: transparent rounded corners
        # (corner alpha 0), an opaque body, and the asset's gray (80,80,80) fill.
        grep -qE 'IconPNG 128x128 corner_alpha=0 opaque_pixels=[1-9][0-9]* gray80_pixels=[1-9]' "$D/tray/props.txt" \
          && pass "icon.png is the app camera asset (corner alpha 0, opaque body, gray 80,80,80)" \
          || fail "icon.png is not the app camera asset (see IconPNG line)"
      else
        fail "IconPixmap PNG conversion failed"
      fi
    else
      fail "SNI property read failed: $(tail -c 120 "$D/tray/props.txt")"
    fi

    if read_layout "$NAME"; then
      note "$(tr '\n' ' ' <"$D/tray/layout.txt" | tr -s ' ' | cut -c1-160)"
      for label in 'Take screenshot' 'Settings' 'Exit'; do
        grep -q "ITEM .* $label\$" "$D/tray/layout.txt" && pass "menu label '$label'" || fail "menu label '$label' missing"
      done
      id_settings=$(item_id 'Settings')
      id_exit=$(item_id 'Exit')
    else
      fail "dbusmenu GetLayout failed: $(tail -c 120 "$D/tray/layout.txt")"
      id_settings=3; id_exit=4
    fi

    # Activate: re-shows the window. Only observable when a WM lets the app
    # hide it (wmctrl -c); without a WM winit already considers the window
    # visible, so its set_visible(true) is a no-op.
    if [ "$WM" = 1 ]; then
      timeout 10 wmctrl -c virshot >/dev/null 2>&1 || true
      wait_map_state IsUnMapped && note "window hidden by WM" || note "window not hidden by WM"
      if timeout 10 dbus-send --print-reply --dest="$NAME" /StatusNotifierItem \
         org.kde.StatusNotifierItem.Activate int32:5 int32:5 >/dev/null 2>&1 \
         && wait_map_state IsViewable; then
        pass "Activate() re-shows hidden window ($(map_state))"
      else
        fail "Activate() did not re-show the window ($(map_state))"
      fi
    else
      timeout 10 dbus-send --print-reply --dest="$NAME" /StatusNotifierItem \
        org.kde.StatusNotifierItem.Activate int32:5 int32:5 >/dev/null 2>&1 \
        && pass "Activate() replied (window stays visible)" || fail "Activate() failed"
      skip "Activate Map-State toggle: without a WM the app never hides the window, so winit's set_visible(true) is a no-op"
    fi

    # dbusmenu Event: settings item re-shows the window (WM mode proves the toggle).
    if [ -n "$id_settings" ]; then
      if [ "$WM" = 1 ]; then
        timeout 10 wmctrl -c virshot >/dev/null 2>&1 || true
        wait_map_state IsUnMapped >/dev/null 2>&1 || true
      fi
      if timeout 20 python3 "$D/tray/sni.py" "$NAME" event "$id_settings" >"$D/tray/event-settings.txt" 2>&1 \
         && wait_map_state IsViewable; then
        pass "menu Event id=$id_settings shows the settings window ($(map_state))"
      else
        fail "menu Event id=$id_settings did not show the settings window ($(map_state))"
      fi
    fi
  }

  assert_tray_exit() {
    local NAME id_exit
    NAME=$(tray_name)
    [ -n "$NAME" ] || { fail "tray bus name gone before Exit test"; return 1; }
    if ! read_layout "$NAME"; then
      fail "dbusmenu GetLayout failed before Exit test: $(tail -c 120 "$D/tray/layout.txt")"
      return 1
    fi
    id_exit=$(item_id 'Exit')
    [ -n "$id_exit" ] || id_exit=4
    if ! timeout 20 python3 "$D/tray/sni.py" "$NAME" event "$id_exit" >"$D/tray/event-exit.txt" 2>&1; then
      fail "menu Event Exit (id=$id_exit) call failed: $(tail -c 120 "$D/tray/event-exit.txt")"
      return 1
    fi
    for _ in $(seq 0 $((TIMEOUT * 2))); do
      kill -0 "$APP" 2>/dev/null || break
      sleep 0.5
    done
    kill -0 "$APP" 2>/dev/null && fail "app still alive after menu Exit" || pass "app exited on menu Exit (id=$id_exit)"
    grep -q 'virshot exiting from tray menu' "$LOG" \
      && { note "$(grep -m1 'virshot exiting from tray menu' "$LOG")"; pass "log: virshot exiting from tray menu"; } \
      || fail "log: 'virshot exiting from tray menu' missing"
    [ -e "$CFG.lock" ] && fail "instance lock still present after menu Exit" || pass "instance lock removed on clean exit"
    APP=""
  }

  assert_wm_extras() {
    timeout 10 wmctrl -l 2>&1 | grep -q 'virshot — Settings' \
      && pass "wmctrl -l lists 'virshot — Settings'" || fail "wmctrl -l does not list the settings window"
    timeout 10 wmctrl -m 2>&1 | grep -q 'Name: Openbox' \
      && pass "wmctrl -m reports Name: Openbox" || fail "wmctrl -m does not report Openbox"
    timeout 10 xdotool getactivewindow >/dev/null 2>&1 \
      && pass "xdotool getactivewindow rc 0 (EWMH focus)" || fail "xdotool getactivewindow failed"
    # Window icon (EWMH): gui::app_window_icon attaches icons::APP_ICON_PNG at
    # window creation (ViewportBuilder::with_icon); winit X11 writes _NET_WM_ICON
    # as CARDINAL [width, height, ARGB pixels...]. Observed here under Xvfb +
    # openbox: xprop prints `_NET_WM_ICON(CARDINAL) = Icon (128 x 128):` and
    # truncates the pixel body ("(not shown)"), so presence + dimensions are what
    # this can pin — the pixel content is covered by the tray IconPixmap check.
    local icon_line
    icon_line=$(timeout 10 xprop -id "$WIN" _NET_WM_ICON 2>&1 | head -1 || true)
    note "$icon_line"
    printf '%s' "$icon_line" | grep -q 'Icon (128 x 128)' \
      && pass "window _NET_WM_ICON present, 128x128 app asset" \
      || fail "window _NET_WM_ICON missing or not 128x128: $icon_line"
    note "$(xwininfo -id "$WIN" 2>/dev/null | grep -E 'Relative upper-left' | tr -s ' ' || true)"
    local attempt rc=1
    for attempt in 1 2 3; do
      timeout 20 env -u WAYLAND_DISPLAY "$BIN" --config "$CFG" --log "$LOG" shot --window \
        --out "$D/shots/cli-window.png" >"$D/shots/cli-shot.txt" 2>&1 && { rc=0; break; }
      sleep 1
    done
    if [ "$rc" = 0 ] && identify -format '%wx%h\n' "$D/shots/cli-window.png" 2>/dev/null | grep -q '720x520'; then
      pass "virshot shot --window rc 0 -> $(cat "$D/shots/cli-shot.txt")"
    else
      fail "virshot shot --window failed after $attempt attempts: $(tail -c 120 "$D/shots/cli-shot.txt")"
    fi
  }

  printf 'mode=%s wm=%s display=%s dbus=%s\n' "$MODE" "$WM" "$DISPLAY" "$DBUS_SESSION_BUS_ADDRESS"
  printf 'dir=%s\n' "$D"
  start_app
  case "$MODE" in
    start) assert_window ;;
    window) assert_window_visual ;;
    shot) assert_hotkey_capture ;;
    log) assert_log ;;
    tray) assert_tray ;;
    stop) assert_tray_exit ;;
    all)
      assert_window_visual
      assert_log
      assert_hotkey_capture
      assert_tray
      if [ "$WM" = 1 ]; then assert_wm_extras; fi
      assert_tray_exit
      ;;
  esac
  if [ "$FAILS" -gt 0 ]; then exit 1; fi
  exit 0
fi

# --- argument parsing -------------------------------------------------------
MODE=""
CONFIG=""
WM=0
KEEP=0
TIMEOUT=20
while [ $# -gt 0 ]; do
  case "$1" in
    start | shot | window | tray | log | stop | all) MODE="$1" ;;
    --config) CONFIG="${2:?--config needs a path}"; shift ;;
    --wm) WM=1 ;;
    --keep) KEEP=1 ;;
    --timeout) TIMEOUT="${2:?--timeout needs a seconds value}"; shift ;;
    -h | --help) sed -n '2,34p' "$0"; exit 0 ;;
    *) echo "usage: $0 [start|shot|window|tray|log|stop|all] [--config P] [--wm] [--keep] [--timeout S]" >&2; exit 2 ;;
  esac
  shift
done
[ -n "$MODE" ] || { echo "usage: $0 [start|shot|window|tray|log|stop|all] [--config P] [--wm] [--keep] [--timeout S]" >&2; exit 2; }

# --- outer mode: scratch dir, Xvfb, D-Bus session, cleanup -------------------
ROOT=$(pwd -P)
TS=$(date +%Y%m%d-%H%M%S)
D="$SCRATCH_ROOT/$TS"
mkdir -p "$D/out" "$D/shots" "$D/tray" "$SCRATCH_ROOT/xdgcache" "$SCRATCH_ROOT/xdgconf"
CFG=${CONFIG:-"$D/config.toml"}
LOG="$D/virshot.log"
if [ -z "$CONFIG" ]; then
  printf '[general]\nsave_path = "%s/out"\n' "$ROOT/$D" >"$CFG"
fi
SAVE_DIR=$(sed -n 's/^save_path[[:space:]]*=[[:space:]]*"\(.*\)".*$/\1/p' "$CFG" 2>/dev/null | head -1)
[ -n "$SAVE_DIR" ] || SAVE_DIR="$D/out"

cat >"$D/tray/sni.py" <<'PY'
#!/usr/bin/env python3
"""SNI + dbusmenu evidence for ui-verify.sh (dbus-python; variants need a proxy)."""
import sys

import dbus

name, mode, arg = sys.argv[1], sys.argv[2], sys.argv[3] if len(sys.argv) > 3 else ""
png = sys.argv[4] if len(sys.argv) > 4 else ""
bus = dbus.SessionBus()
IFACE = "org.kde.StatusNotifierItem"

if mode == "props":
    props = dbus.Interface(bus.get_object(name, "/StatusNotifierItem"), "org.freedesktop.DBus.Properties")
    tip = props.Get(IFACE, "ToolTip")
    print(f"ToolTip title={str(tip[2])!r} description={str(tip[3])!r}")
    for pixmap in props.Get(IFACE, "IconPixmap"):
        width, height, data = int(pixmap[0]), int(pixmap[1]), bytes(pixmap[2])
        print(f"IconPixmap {width}x{height} bytes={len(data)}")
        with open(arg, "wb") as f:
            f.write(data)
        if png:
            # SNI IconPixmap is network-order ARGB (tray-icon ksni rotates each
            # RGBA pixel); ImageMagick has no ARGB raw reader, so reorder with
            # PIL and save a real alpha PNG.
            from PIL import Image

            rgba = bytearray(len(data))
            rgba[0::4] = data[1::4]
            rgba[1::4] = data[2::4]
            rgba[2::4] = data[3::4]
            rgba[3::4] = data[0::4]
            img = Image.frombytes("RGBA", (width, height), bytes(rgba))
            img.save(png)
            opaque = gray80 = 0
            for pixel in img.getdata():
                if pixel[3] == 255:
                    opaque += 1
                if pixel == (80, 80, 80, 255):
                    gray80 += 1
            print(
                f"IconPNG {width}x{height} corner_alpha={img.getpixel((0, 0))[3]} "
                f"opaque_pixels={opaque} gray80_pixels={gray80}"
            )
elif mode == "layout":
    menu = dbus.Interface(bus.get_object(name, "/MenuBar"), "com.canonical.dbusmenu")
    revision, layout = menu.GetLayout(0, -1, [])
    print(f"GetLayout revision={revision}")

    def walk(items):
        for item in items:
            properties = dict(item[1])
            label = str(properties.get("label", "")) or str(properties.get("type", "separator"))
            print(f"ITEM {int(item[0])} {label}")
            walk(item[2])

    walk([layout])
elif mode == "event":
    menu = dbus.Interface(bus.get_object(name, "/MenuBar"), "com.canonical.dbusmenu")
    menu.AboutToShow(0)
    menu.Event(int(arg), "clicked", dbus.String(""), dbus.UInt32(0))
    print(f"Event id={arg} key=clicked sent")
PY

cleanup_outer() {
  local rc=$?
  local pid ob survivors
  pid=$(cat "$D/app.pid" 2>/dev/null || true)
  if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
    kill -TERM "$pid" 2>/dev/null || true
    for _ in 1 2 3 4 5 6 7 8 9 10; do
      kill -0 "$pid" 2>/dev/null || break
      sleep 0.5
    done
  fi
  ob=$(cat "$D/openbox.pid" 2>/dev/null || true)
  if [ -n "$ob" ] && kill -0 "$ob" 2>/dev/null; then kill -TERM "$ob" 2>/dev/null || true; fi
  rm -f "$CFG.lock"
  # xvfb-run/dbus-run-session tear down their own children asynchronously; settle
  # before checking, then kill anything still ours and check again.
  survivors=$(pgrep -af 'virshot|Xvfb|dbus-daemon|openbox' || true)
  for _ in 1 2 3 4 5 6 7 8; do
    [ -z "$survivors" ] && break
    sleep 0.5
    survivors=$(pgrep -af 'virshot|Xvfb|dbus-daemon|openbox' || true)
  done
  if [ -n "$survivors" ]; then
    for pid in $(printf '%s\n' "$survivors" | awk '{print $1}'); do
      kill -TERM "$pid" 2>/dev/null || true
    done
    for _ in 1 2 3 4 5 6; do
      survivors=$(pgrep -af 'virshot|Xvfb|dbus-daemon|openbox' || true)
      [ -z "$survivors" ] && break
      sleep 0.5
    done
  fi
  if [ -n "$survivors" ]; then
    printf 'FAIL leftover processes: %s\n' "$(printf '%s' "$survivors" | tr '\n' ' ')"
    rc=1
  else
    printf 'PASS no leftover virshot/Xvfb/dbus-daemon/openbox processes\n'
  fi
  if [ "$KEEP" = 1 ]; then
    printf 'artifacts kept: %s/config.toml %s/virshot.log %s/out %s/shots %s/tray\n' "$D" "$D" "$D" "$D" "$D"
  else
    rm -rf "$D"
    printf 'scratch dir removed: %s\n' "$D"
  fi
  exit "$rc"
}
trap cleanup_outer EXIT INT TERM

env -u WAYLAND_DISPLAY \
  XDG_CACHE_HOME="$ROOT/$SCRATCH_ROOT/xdgcache" \
  XDG_CONFIG_HOME="$ROOT/$SCRATCH_ROOT/xdgconf" \
  UIVERIFY_INNER=1 UIVERIFY_MODE="$MODE" UIVERIFY_DIR="$ROOT/$D" \
  UIVERIFY_CONFIG="$CFG" UIVERIFY_LOG="$LOG" UIVERIFY_WM="$WM" \
  UIVERIFY_TIMEOUT="$TIMEOUT" UIVERIFY_SAVE_DIR="$SAVE_DIR" \
  xvfb-run -a --server-args="-screen 0 1280x1024x24" dbus-run-session -- "$ROOT/$0"
# Propagate the inner assertion count (the trap re-reads $? on EXIT).
exit $?
