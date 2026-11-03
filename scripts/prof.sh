#!/usr/bin/env bash
# prof.sh — memory/CPU profiling workflow helper for virshot.
#
# One invocation = one Xvfb display + one scratch dir ./.tmp/prof/<ts>/ holding
# the scratch config, log, captures and all profile artifacts. valgrind/perf run
# INSIDE the inner command (wrapping xvfb-run under valgrind profiles the
# wrapper's forked child instead of the app, because trace-children=no).
#
# Subcommands and what they produce:
#   heap       valgrind --tool=massif (looped `virshot shot` captures) ->
#              massif-<i>.out per iteration. Prints the peak mem_heap_B (max
#              snapshot value over all iterations), per-iteration peaks/snapshot
#              counts, and the fast-build/performance-statistics block if
#              massif used fast-build mode. Read: massif.out is a plain text
#              dump of `mem_heap_B=` per snapshot (msprint/msreport are NOT
#              installed here, so this script parses it with awk/grep);
#              `heap_tree=detailed` blocks show who allocated what.
#   leak       valgrind --tool=memcheck on three exit-naturally paths:
#              --help, --version, and a headless `virshot shot` -> mc-<path>.txt
#              (full memcheck output). Prints the LEAK SUMMARY + ERROR SUMMARY
#              lines of each run. PASS = "definitely lost: 0 bytes";
#              indirectly/possibly/unreachable counts are printed as
#              information, never a failure (the logging worker thread's TLS
#              block shows up as "possibly lost" and is not a leak we own).
#   cpu        perf record -F 99 --call-graph dwarf,64000 -> perf.data of the
#              looped headless shots, then perf report --stdio (top functions)
#              and perf script (first samples). Asserts perf.data exists and
#              that report lines carry real function names, not bare addresses.
#              Read: `perf report -i ./.tmp/prof/<ts>/perf.data --stdio`, or
#              `perf script` to dump raw stacks; no SVG/flamegraph tool is
#              installed on this host, so --stdio/stdout analysis is the only
#              route.
#   cpu --resident
#              Optional variant: records the bare resident `virshot` under Xvfb
#              for --timeout seconds, then stops it with SIGTERM. perf writes
#              its data on signal, so the report is PARTIAL (startup + event
#              loop only). valgrind cannot profile this variant: a forced
#              SIGTERM kills the guest and no report is written at all, so
#              heap/leak always use the headless `virshot shot` path instead.
#   mem-count  perf stat -e mem_inst_retired.all_loads,mem_inst_retired.all_stores
#              on the looped shots -> perf-stat.txt counter table. PASS = loads > 0.
#   all        heap + leak + cpu + mem-count in one scratch dir and one display.
#
# Options:
#   --iterations N   looped captures per profile (default 5)
#   --config <path>  use this config instead of the generated scratch config
#   --timeout <s>    recording budget for cpu --resident (default 20)
#   --resident       resident-app CPU variant (see above)
#   --keep           keep the scratch dir (it is kept by default; .tmp is git-ignored)
#   --clean          delete the scratch dir on exit
#
# Examples:
#   ./scripts/prof.sh heap --iterations 3 --keep
#   ./scripts/prof.sh leak
#   ./scripts/prof.sh cpu --resident --timeout 12 --keep
#   ./scripts/prof.sh all --config ./config.toml
#
# Host constraints this script encodes:
#   - debug binary only: ./target/debug/virshot. The release profile is
#     opt-level="z", lto="thin", strip=true -> no symbols, so perf/valgrind
#     analysis of the release binary reports addresses, not functions.
#   - perf_event_paranoid=2: sampling own processes works without root, but
#     `perf mem record` fails on this host ("memory events not supported") and
#     plain cache/TLB sampling events fail ("PMU Hardware doesn't support
#     sampling/overflow-intervals") -> never use `perf mem`; memory traffic is
#     measured by `perf stat` COUNTING (mem_inst_retired.* works, exit 0).
#   - valgrind needs --massif-out-file=<path> (its default lands in CWD) and is
#     ~14x slower than native.
#   - xcap capture under Xvfb is flaky (~25-40% success): each shot retries up
#     to 3 attempts; if all retries fail the profile is still produced, because
#     the headless failure path allocates too.
#   - No `virshot doctor` subcommand: memcheck uses --help, --version, shot.
#   - valgrind/perf have no `--time-in-seconds`; use shell-side timing only.
#   - `<config>.lock` is a pid lock left stale after SIGTERM -> removed on exit.
set -euo pipefail

script_dir="$(dirname "$0")"
here="$(cd "$script_dir/.." && pwd)"
cd "$here"

BIN=./target/debug/virshot
SCRATCH_ROOT=./.tmp/prof
RETRIES=3
DEFAULT_TIMEOUT=20
XVFB_ARGS="-screen 0 1280x1024x24"

# --- inner mode: runs inside the Xvfb display ------------------------------
if [ "${PROF_INNER:-0}" = "1" ]; then
  D="$PROF_DIR"
  CFG="$PROF_CONFIG"
  LOG="$PROF_LOG"
  SHOTS="$PROF_SHOTS"
  ITER="$PROF_ITER"
  TIMEOUT="$PROF_TIMEOUT"
  RESIDENT="$PROF_RESIDENT"
  MODE="$PROF_MODE"
  FAILS=0

  pass() { printf 'PASS %s\n' "$1"; }
  fail() { printf 'FAIL %s\n' "$1"; FAILS=$((FAILS + 1)); }
  note() { printf '     %s\n' "$1"; }

  # The recorded loop body: one shot per iteration, RETRIES attempts each.
  # perf wraps this script, so its children are the profiled workload.
  write_shot_loop() {
    cat > "$D/shot-loop.sh" <<'LOOP'
#!/usr/bin/env bash
set -uo pipefail
ok=0; failed=0
for i in $(seq 1 "${PROF_ITER:-1}"); do
  a=1
  while [ "$a" -le "${PROF_RETRIES:-3}" ]; do
    if env -u WAYLAND_DISPLAY "${PROF_BIN:-./target/debug/virshot}" shot \
         --config "$PROF_CONFIG" --log "$PROF_LOG" \
         --out "${PROF_SHOTS}/shot-$i.png" >/dev/null 2>&1; then
      ok=$((ok + 1)); break
    fi
    a=$((a + 1))
  done
  if [ "$a" -gt "${PROF_RETRIES:-3}" ]; then failed=$((failed + 1)); fi
done
printf 'shots ok=%d failed=%d\n' "$ok" "$failed"
LOOP
  }

  # Shared symbol check for a perf report file.
  report_symbols() {
    local f="$1"
    local named unknown
    named="$(grep -cE '\[.\] [A-Za-z_]' "$f" 2>/dev/null || true)"
    unknown="$(grep -c '\[unknown\]' "$f" 2>/dev/null || true)"
    if [ "${named:-0}" -gt 0 ]; then
      pass "symbols resolve (${named} report line(s) with function names)"
    else
      fail "no symbols resolved in $(basename "$f") (addresses only)"
    fi
    if [ "${unknown:-0}" -gt 0 ]; then
      note "${unknown} '[unknown]' line(s): this perf build lacks libdw/libunwind, dwarf stacks can be partial"
    fi
  }

  inner_heap() {
    printf '\n== heap: valgrind massif, %s iteration(s) ==\n' "$ITER"
    local peak=0 best="" snapshots=0 iter_peak iter_snaps ok_any=0
    for i in $(seq 1 "$ITER"); do
      local mf="$D/massif-$i.out" rc=0 ok=0
      for a in $(seq 1 "$RETRIES"); do
        if env -u WAYLAND_DISPLAY valgrind --tool=massif --time-unit=ms \
             --massif-out-file="$mf" "$BIN" shot --config "$CFG" --log "$LOG" \
             --out "$SHOTS/heap-$i.png" > "$D/heap-$i.log" 2>&1; then
          ok=1; break
        fi
      done
      if [ "$ok" = "1" ]; then ok_any=1; fi
      iter_snaps="$(grep -c '^mem_heap_B=' "$mf" 2>/dev/null || true)"
      iter_peak="$(awk -F= '/^mem_heap_B=/ { v = $2 + 0; if (v > max) max = v } END { print max + 0 }' "$mf" 2>/dev/null || true)"
      if [ "${iter_peak:-0}" -gt "$peak" ]; then peak="$iter_peak"; best="iteration $i"; fi
      snapshots=$((snapshots + ${iter_snaps:-0}))
      note "iter $i: peak mem_heap_B=${iter_peak:-0}, snapshots=${iter_snaps:-0}, capture $(
        [ "$ok" = "1" ] && echo ok || echo 'failed (all retries)')"
    done
    if [ "$peak" -gt 0 ]; then
      pass "heap: peak mem_heap_B=$peak ($best, $snapshots snapshots total)"
    else
      fail "heap: no mem_heap_B snapshot found in massif.out"
    fi
    if [ "$ok_any" = "0" ]; then
      note "no capture succeeded under massif; the profile covers headless failure paths"
    fi
    local fb
    fb="$(grep -nE 'fast-build|performance statistics' "$D"/massif-*.out 2>/dev/null | head -12 || true)"
    if [ -n "$fb" ]; then
      printf '     fast-build block:\n'
      printf '%s\n' "$fb" | sed 's/^/     /'
    else
      note "fast-build block: not present (massif stayed in slow-build mode)"
    fi
  }

  inner_leak() {
    printf '\n== leak: valgrind memcheck (--help, --version, headless shot) ==\n'
    local total_def=0
    mc_run() {
      local label="$1"
      local rc=0 attempt=0 out="$D/mc-$label.txt"
      shift
      while [ "$attempt" -lt "$RETRIES" ]; do
        attempt=$((attempt + 1))
        rc=0
        env -u WAYLAND_DISPLAY valgrind --tool=memcheck --error-exitcode=1 \
          --leak-check=full --show-leak-kinds=all "$BIN" "$@" > "$out" 2>&1 || rc=$?
        if [ "$label" != "shot" ] || [ "$rc" = "0" ]; then break; fi
      done
      printf '  -- %s (valgrind rc=%s, attempt %s/%s)\n' "$label" "$rc" "$attempt" "$RETRIES"
      grep -E 'LEAK SUMMARY|definitely lost|indirectly lost|possibly lost|unreachable|ERROR SUMMARY|in total:' "$out" 2>/dev/null \
        | sed 's/^/     /' || true
      local def
      def="$(grep -oE 'definitely lost: [0-9]+ bytes' "$out" 2>/dev/null | head -1 | grep -oE '[0-9]+' || true)"
      if [ -z "$def" ]; then
        fail "leak/$label: no LEAK SUMMARY line in $(basename "$out")"
      elif [ "$def" = "0" ]; then
        pass "leak/$label: definitely lost 0 bytes"
      else
        fail "leak/$label: definitely lost $def bytes"
      fi
      total_def=$((total_def + ${def:-0}))
    }
    mc_run help --help
    mc_run version --version
    mc_run shot shot --config "$CFG" --log "$LOG" --out "$SHOTS/leak.png"
    if [ "$total_def" -eq 0 ]; then
      pass "leak: 0 bytes definitely lost across all paths"
    fi
  }

  inner_cpu() {
    printf '\n== cpu: perf record -F 99 --call-graph dwarf,64000, %s iteration(s) ==\n' "$ITER"
    write_shot_loop
    local rc=0
    perf record -F 99 --call-graph dwarf,64000 -o "$D/perf.data" -- \
      bash "$D/shot-loop.sh" > "$D/perf-record.log" 2>&1 || rc=$?
    note "perf record rc=$rc (non-zero when a capture attempt fails; data is still valid)"
    if [ -s "$D/perf.data" ]; then
      pass "cpu: perf.data exists ($(wc -c < "$D/perf.data") bytes)"
    else
      fail "cpu: perf.data missing or empty"
      return
    fi
    perf report --stdio -i "$D/perf.data" > "$D/perf-report.txt" 2>&1 || true
    printf '  -- perf report --stdio (head):\n'
    head -25 "$D/perf-report.txt"
    perf script -i "$D/perf.data" > "$D/perf-script.txt" 2>&1 || true
    printf '  -- perf script (head):\n'
    head -20 "$D/perf-script.txt"
    grep -m1 '^# Samples:' "$D/perf-report.txt" 2>/dev/null | sed 's/^/     /' || true
    report_symbols "$D/perf-report.txt"
  }

  inner_cpu_resident() {
    printf '\n== cpu --resident: bare virshot under Xvfb for %ss, stopped with SIGTERM ==\n' "$TIMEOUT"
    printf '     PARTIAL report (perf writes its data on signal); valgrind cannot profile this run.\n'
    local rc=0 app=""
    perf record -F 99 --call-graph dwarf,64000 -o "$D/perf-resident.data" -- \
      env -u WAYLAND_DISPLAY "$BIN" --config "$CFG" --log "$LOG" \
      > "$D/perf-resident.log" 2>&1 &
    local ppid=$!
    sleep "$TIMEOUT"
    app="$(pgrep -x virshot | head -1 || true)"
    if [ -n "$app" ]; then kill -TERM "$app" 2>/dev/null || true
    else kill -TERM "$ppid" 2>/dev/null || true; fi
    wait "$ppid" 2>/dev/null || true
    if [ -s "$D/perf-resident.data" ]; then
      pass "cpu --resident: perf-resident.data exists ($(wc -c < "$D/perf-resident.data") bytes)"
      perf report --stdio -i "$D/perf-resident.data" > "$D/perf-resident-report.txt" 2>&1 || true
      printf '  -- perf report --stdio (head):\n'
      head -25 "$D/perf-resident-report.txt"
      perf script -i "$D/perf-resident.data" > "$D/perf-resident-script.txt" 2>&1 || true
      printf '  -- perf script (head):\n'
      head -20 "$D/perf-resident-script.txt"
      report_symbols "$D/perf-resident-report.txt"
    else
      fail "cpu --resident: perf-resident.data missing or empty (rc=$rc)"
    fi
  }

  inner_memcount() {
    printf '\n== mem-count: perf stat counting, %s iteration(s) ==\n' "$ITER"
    write_shot_loop
    local rc=0 loads="" stores=""
    perf stat -e mem_inst_retired.all_loads,mem_inst_retired.all_stores -- \
      bash "$D/shot-loop.sh" > "$D/perf-stat.txt" 2>&1 || rc=$?
    note "perf stat rc=$rc (non-zero when a capture attempt fails; counters are still reported)"
    printf '  -- counter table:\n'
    grep -E 'mem_inst_retired|seconds time elapsed|Performance counter stats' "$D/perf-stat.txt" \
      | sed 's/^/     /' || true
    loads="$(awk '/mem_inst_retired.all_loads/ { gsub(/,/, "", $1); print $1 }' "$D/perf-stat.txt" 2>/dev/null || true)"
    stores="$(awk '/mem_inst_retired.all_stores/ { gsub(/,/, "", $1); print $1 }' "$D/perf-stat.txt" 2>/dev/null || true)"
    printf '     loads=%s stores=%s\n' "${loads:-0}" "${stores:-0}"
    if [ "${loads:-0}" -gt 0 ]; then
      pass "mem-count: ${loads} retired load instructions (> 0)"
    else
      fail "mem-count: no retired load instructions counted"
    fi
  }

  case "$MODE" in
    heap) inner_heap ;;
    leak) inner_leak ;;
    cpu)
      if [ "$RESIDENT" = "1" ]; then inner_cpu_resident; else inner_cpu; fi
      ;;
    mem-count) inner_memcount ;;
    all)
      inner_heap; inner_leak
      if [ "$RESIDENT" = "1" ]; then inner_cpu_resident; else inner_cpu; fi
      inner_memcount
      ;;
    *) printf 'FAIL inner: unknown mode %s\n' "$MODE"; FAILS=$((FAILS + 1)) ;;
  esac

  printf '\ninner: %s failure(s)\n' "$FAILS"
  if [ "$FAILS" -gt 0 ]; then exit 1; fi
  exit 0
fi

# --- outer mode ------------------------------------------------------------
usage() {
  sed -n '2,60p' "$0" | sed 's/^# \{0,1\}//'
}

MODE=""
ITER=5
CFG=""
TIMEOUT="$DEFAULT_TIMEOUT"
CLEAN=0
RESIDENT=0

while [ $# -gt 0 ]; do
  case "$1" in
    heap|leak|cpu|mem-count|all) MODE="$1" ;;
    --keep) CLEAN=0 ;;
    --clean) CLEAN=1 ;;
    --resident) RESIDENT=1 ;;
    --iterations) ITER="${2:-}"; shift ;;
    --config) CFG="${2:-}"; shift ;;
    --timeout) TIMEOUT="${2:-}"; shift ;;
    -h|--help) usage; exit 0 ;;
    *) printf 'prof.sh: unknown argument %s\n' "$1" >&2; exit 2 ;;
  esac
  shift
done

if [ -z "$MODE" ]; then
  printf 'prof.sh: need a subcommand: heap | leak | cpu | mem-count | all\n' >&2
  exit 2
fi
case "$ITER" in ''|*[!0-9]*) printf 'prof.sh: --iterations must be a positive integer\n' >&2; exit 2 ;; esac
case "$TIMEOUT" in ''|*[!0-9]*) printf 'prof.sh: --timeout must be a positive integer\n' >&2; exit 2 ;; esac
if [ "$ITER" -lt 1 ] || [ "$TIMEOUT" -lt 1 ]; then
  printf 'prof.sh: --iterations and --timeout must be >= 1\n' >&2
  exit 2
fi

TS="$(date +%Y%m%d-%H%M%S)"
D="$SCRATCH_ROOT/$TS"
mkdir -p "$D/shots"

if [ -z "$CFG" ]; then
  CFG="$D/config.toml"
  printf '[general]\nsave_path = "%s"\n' "$D/shots" > "$CFG"
fi

printf 'prof.sh: mode=%s iterations=%s timeout=%ss resident=%s\n' "$MODE" "$ITER" "$TIMEOUT" "$RESIDENT"
printf 'prof.sh: scratch dir %s, config %s, log %s\n' "$D" "$CFG" "$D/prof.log"

export PROF_DIR="$D" PROF_CONFIG="$CFG" PROF_LOG="$D/prof.log" PROF_SHOTS="$D/shots"
export PROF_ITER="$ITER" PROF_TIMEOUT="$TIMEOUT" PROF_MODE="$MODE" PROF_RESIDENT="$RESIDENT"
export PROF_BIN="$BIN" PROF_RETRIES="$RETRIES" PROF_INNER=1

rc=0
env -u WAYLAND_DISPLAY xvfb-run -a --server-args="$XVFB_ARGS" bash "$0" || rc=$?

cleanup() {
  # Match process names, not command lines: a caller whose own command line
  # mentions virshot/Xvfb must never be signalled here.
  pkill -TERM -x virshot >/dev/null 2>&1 || true
  pkill -TERM -x Xvfb >/dev/null 2>&1 || true
  rm -f "$CFG.lock" 2>/dev/null || true
  if [ "$CLEAN" = "1" ]; then rm -rf "$D" 2>/dev/null || true; fi
}
cleanup

left="$( { pgrep -ax virshot || true; pgrep -ax Xvfb || true; } 2>/dev/null )"
if [ -n "$left" ]; then
  printf 'prof.sh: leftover processes:\n%s\n' "$left" >&2
  pkill -KILL -x virshot >/dev/null 2>&1 || true
  pkill -KILL -x Xvfb >/dev/null 2>&1 || true
  rc=1
fi

if [ "$CLEAN" = "1" ]; then
  printf 'prof.sh: scratch dir %s removed (--clean)\n' "$D"
else
  printf 'prof.sh: artifacts kept under %s\n' "$D"
  ls -1 "$D" | sed 's/^/  /'
fi
printf 'prof.sh: rc=%s\n' "$rc"
exit "$rc"
