#!/usr/bin/env bash
# Runs probes on an emulator that starts from a known-idle snapshot, never from a cold boot.
#
# A cold boot is a ~40 s CPU burst during which two things go wrong (docs/probes/p5.md):
# resource overlays are applied after boot_completed (CONFIG_ASSETS_PATHS relaunches the activity
# and SDL3 does not survive it), and System UI's startup can race a CPU-bound SurfaceFlinger and
# leave a "System UI isn't responding" dialog that stays until dismissed. So: cold-boot once, wait
# until the system is genuinely idle and clean, save a snapshot; every run starts from it.
#
# usage: emulator.sh prepare AVD    cold-boot, wait until idle and clean, save snapshot leonis-ready
#        emulator.sh start AVD      boot from leonis-ready (not saved back), check it is clean
#        emulator.sh stop
# exit:  0 done, 1 not clean after all attempts, 2 could not run
set -euo pipefail

SNAPSHOT=leonis-ready
sdk=${ANDROID_HOME:-$HOME/Android/Sdk}
adb=$sdk/platform-tools/adb
emulator=$sdk/emulator/emulator
log=${EMULATOR_LOG:-${TMPDIR:-/tmp}/leonis-emulator.log}
[ -x "$adb" ] && [ -x "$emulator" ] || { echo "emulator.sh: SDK tools not found under $sdk" >&2; exit 2; }
# Snapshots only load with the same hardware settings they were saved with: one set, used by both.
flags=(-no-window -gpu swiftshader_indirect -no-audio -no-boot-anim -memory 3072)

stop() { "$adb" emu kill >/dev/null 2>&1 || true; sleep 3; }

# Waits for adb and sys.boot_completed; returns 1 if the emulator process died, 2 on timeout.
wait_boot() {
  local pid=$1 i
  for i in $(seq 1 120); do
    kill -0 "$pid" 2>/dev/null || { echo "emulator.sh: emulator exited; log:" >&2; tail -40 "$log" >&2; return 1; }
    [ "$("$adb" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = 1 ] && { echo "  booted after ~$((i * 2)) s"; return 0; }
    sleep 2
  done
  echo "emulator.sh: not booted after 240 s" >&2
  return 2
}

focus() { "$adb" shell dumpsys window | grep -o 'mCurrentFocus=.*' | head -1 | tr -d '\r'; }
dialog_up() { [[ "$(focus)" == *"Not Responding"* ]]; }

# CPU busy % over the last ~2 s from /proc/stat.
cpu_busy() {
  local a b
  a=$("$adb" shell head -1 /proc/stat | tr -d '\r'); sleep 2; b=$("$adb" shell head -1 /proc/stat | tr -d '\r')
  awk -v a="$a" -v b="$b" 'BEGIN {
    split(a, x); split(b, y); t = 0; i = 0
    for (k = 2; k <= 9; k++) t += y[k] - x[k]
    i = (y[5] - x[5]) + (y[6] - x[6])
    print (t > 0) ? int(100 * (t - i) / t) : 100 }'
}

# Idle and clean: launcher focused, no dialog, no configuration change and CPU < 15% busy, all held
# for 10 s. Returns 1 if a "not responding" dialog appears (the boot is discarded, never dismissed),
# 2 if not idle within 180 s.
wait_idle() {
  local waited=0 calm=0 busy now last f
  while [ "$waited" -lt 180 ]; do
    f=$(focus)
    if [[ "$f" == *"Not Responding"* ]]; then
      echo "  dialog: ${f#mCurrentFocus=}"
      return 1
    fi
    busy=$(cpu_busy)
    now=$("$adb" shell date +%s | tr -d '\r')
    last=$("$adb" logcat -d -v epoch -s ActivityTaskManager:I | grep 'Config changes=' | tail -1 \
      | awk '{print int($1)}')
    if [[ "$f" == *launcher* ]] && [ "$busy" -lt 15 ] && { [ -z "$last" ] || [ $((now - last)) -ge 10 ]; }; then
      calm=$((calm + 2))
    else
      calm=0
    fi
    if [ "$calm" -ge 10 ]; then
      local since="none logged"
      [ -n "$last" ] && since="$((now - last)) s ago"
      echo "  idle: cpu ${busy}% busy, launcher focused, last config change ${since}; waited ${waited} s"
      return 0
    fi
    sleep 1
    waited=$((waited + 3))
  done
  echo "emulator.sh: not idle after 180 s (cpu ${busy:-?}%, focus ${f:-?})" >&2
  return 2
}

case "${1:-}" in
  prepare)
    avd=${2:?usage: emulator.sh prepare AVD}
    for attempt in 1 2 3; do
      stop
      echo "prepare: cold boot $attempt of 3"
      nohup "$emulator" -avd "$avd" "${flags[@]}" -no-snapshot-load -no-snapshot-save > "$log" 2>&1 &
      wait_boot $! || continue
      if wait_idle; then
        "$adb" emu avd snapshot save "$SNAPSHOT" | tr -d '\r' | grep -v '^OK$' || true
        "$adb" emu avd snapshot list | tr -d '\r' | grep -q "$SNAPSHOT" \
          || { echo "emulator.sh: snapshot $SNAPSHOT not listed after save" >&2; stop; exit 1; }
        echo "prepare: saved snapshot $SNAPSHOT"
        stop
        exit 0
      fi
      echo "prepare: boot $attempt not clean; discarding it"
    done
    stop
    echo "emulator.sh: no clean boot in 3 attempts" >&2
    exit 1
    ;;
  start)
    avd=${2:?usage: emulator.sh start AVD}
    stop
    nohup "$emulator" -avd "$avd" "${flags[@]}" -snapshot "$SNAPSHOT" -no-snapshot-save > "$log" 2>&1 &
    pid=$!
    wait_boot "$pid" || exit 2
    grep -q "Successfully loaded snapshot '$SNAPSHOT'\|Loading snapshot '$SNAPSHOT'" "$log" \
      || echo "  note: the emulator log does not mention loading $SNAPSHOT"
    if dialog_up; then echo "emulator.sh: snapshot started with a dialog up: $(focus)" >&2; exit 1; fi
    f=$(focus)
    echo "start: from $SNAPSHOT, focus ${f#mCurrentFocus=}"
    ;;
  stop) stop ;;
  *) echo "usage: emulator.sh prepare AVD | start AVD | stop" >&2; exit 2 ;;
esac
