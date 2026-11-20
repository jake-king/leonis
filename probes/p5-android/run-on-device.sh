#!/usr/bin/env bash
# Installs the P5 APK on the one connected device/emulator, runs the probe and its negative control,
# and checks both verdicts from logcat. Optionally saves a mid-run screenshot.
#
# usage: run-on-device.sh APK [SCREENSHOT.png]
# exit:  0 probe passed and control failed as it must, 1 otherwise, 2 could not run
set -euo pipefail

apk=${1:?usage: run-on-device.sh APK [SCREENSHOT.png]}
shot=${2:-}
adb=${ADB:-${ANDROID_HOME:-$HOME/Android/Sdk}/platform-tools/adb}
[ -x "$adb" ] || { echo "run-on-device: adb not found at $adb" >&2; exit 2; }
[ -f "$apk" ] || { echo "run-on-device: no APK at $apk" >&2; exit 2; }
[ "$("$adb" get-state 2>/dev/null)" = device ] || { echo "run-on-device: no device" >&2; exit 2; }

# Right after boot an emulator is not ready even when sys.boot_completed=1 (docs/probes/p5.md):
# - resource overlays are still applied on user unlock; each is a CONFIG_ASSETS_PATHS change that
#   relaunches the activity, and SDL3 does not survive a relaunch;
# - System UI can be starved for CPU and show "System UI isn't responding", covering the app.
# Settled means: no configuration change logged for 10 s AND the launcher has held focus, with no
# "not responding" dialog, for 10 s. Up to 180 s.
focus() { "$adb" shell dumpsys window | grep -o 'mCurrentFocus=.*' | head -1; }
settle() {
  local waited=0 now last f calm=0
  while [ "$waited" -lt 180 ]; do
    now=$("$adb" shell date +%s | tr -d '\r')
    last=$("$adb" logcat -d -v epoch -s ActivityTaskManager:I | grep 'Config changes=' | tail -1 \
      | awk '{print int($1)}')
    f=$(focus)
    if [[ "$f" == *launcher* ]] && [[ "$f" != *"Not Responding"* ]]; then
      calm=$((calm + 2))
    else
      [ "$calm" -gt 0 ] && echo "  settle: focus left the launcher: $f"
      calm=0
    fi
    if { [ -z "$last" ] || [ $((now - last)) -ge 10 ]; } && [ "$calm" -ge 10 ]; then
      echo "  settled: last configuration change ${last:+$((now - last)) s ago}, launcher focused ${calm} s, waited ${waited} s"
      return 0
    fi
    sleep 2
    waited=$((waited + 2))
  done
  echo "run-on-device: not settled after 180 s (focus: $(focus))" >&2
  return 2
}

settle || exit 2
"$adb" install -r "$apk" >/dev/null

# Prints which window has focus and which activity is resumed: what owns the foreground.
foreground() {
  echo "  foreground ($1):"
  "$adb" shell dumpsys window | grep -E 'mCurrentFocus|mFocusedApp' | sed 's/^ */    /' | head -4
  "$adb" shell dumpsys activity activities | grep -E 'topResumedActivity|ResumedActivity:' \
    | sed 's/^ */    /' | head -3
}

# Runs the probe once with the given mode; prints its result line; returns the probe's exit code
# (2 if no verdict appeared within the timeout).
run() {
  local mode=$1 save=${2:-}
  "$adb" logcat -c
  "$adb" shell setprop debug.leonis.p5 "${mode:-\'\'}"
  foreground "before launch, mode '${mode}'"
  "$adb" shell am start -S -n org.leonis.p5/org.libsdl.app.SDLActivity >/dev/null
  if [ -n "$save" ]; then
    # Screenshot only once the probe has presented: a fixed delay caught the launch animation on CI.
    for _ in $(seq 1 60); do
      "$adb" logcat -d -s leonis:I | grep -q 'p5 first_frame' && break
      sleep 0.5
    done
    # First-frame is not on-screen yet: Android may still be animating the launch and the rotation.
    # The probe draws its colour on every frame, so take screenshots until one shows it (10 tries,
    # 0.5 s apart, while the probe runs); the check after the runs fails if none did.
    local try
    for try in $(seq 1 10); do
      sleep 0.5
      "$adb" exec-out screencap -p > "$save"
      python3 -I "$(dirname "$0")/screen_pixel.py" "$save" 0,188,255 >/dev/null 2>&1 && break
    done
    echo "  screenshot: try $try of 10"
    shot_focus=$(focus)
  fi
  local log=""
  for _ in $(seq 1 45); do
    log=$("$adb" logcat -d -s leonis:I)
    grep -q 'p5 exit=' <<<"$log" && break
    sleep 2
  done
  # Every line the probe logged (setup, lifecycle events, verdict) plus SDL's own lines.
  grep -E 'p5 ' <<<"$log" | sed -E 's/^.*leonis *: //'
  "$adb" logcat -d -s SDL:V SDL/APP:V SDLActivity:V | grep -viE 'beginning of' | sed -E 's/^.{18}/  sdl: /' | tail -25
  local code
  code=$(grep -oE 'p5 exit=[0-9]+' <<<"$log" | tail -1 | cut -d= -f2)
  if [ -z "$code" ]; then
    echo "run-on-device: no verdict from mode '${mode}' in 90 s" >&2
    "$adb" logcat -d > "${LOGCAT_DIR:-.}/logcat-timeout-${mode:-normal}.txt" 2>&1 || true
    echo "  full logcat saved to ${LOGCAT_DIR:-.}/logcat-timeout-${mode:-normal}.txt" >&2
    foreground "at timeout" >&2
    if "$adb" logcat -d | grep -q 'org.leonis.p5/org.libsdl.app.SDLActivity.* is relaunching'; then
      echo "  the probe's activity was RELAUNCHED (a configuration change): SDL3 does not survive that" >&2
    fi
    "$adb" logcat -d | grep -E 'ActivityTaskManager|ActivityManager: (Start|Kill|Process)|\bANR\b|not responding' \
      | sed -E 's/^.{18}/  am: /' | tail -25 >&2 || true
    echo "  crash lines:" >&2
    "$adb" logcat -d | grep -E 'FATAL|AndroidRuntime|DEBUG   :' | head -20 >&2 || true
    return 2
  fi
  return "$code"
}

set +e
run "" "$shot"; probe=$?
run plant-wrong-colour; control=$?
set -e
echo "probe exit=$probe control exit=$control"
if [ -n "$shot" ]; then
  # The screenshot is evidence only if it shows the probe's colour. If a system "not responding"
  # dialog covered the screen, the run never had a fair chance: exit 2 (could not run), named.
  python3 -I "$(dirname "$0")/screen_pixel.py" "$shot" 0,188,255 || screen=$?
  if [ "${screen:-0}" -ne 0 ]; then
    # Classified from the focused window at the last screenshot (the name seen in CI's output:
    # "Application Not Responding: com.android.systemui").
    echo "  focus at the last screenshot: ${shot_focus:-unknown}" >&2
    if [[ "${shot_focus:-}" == *"Not Responding"* ]]; then
      echo "run-on-device: screenshot covered: System UI was not responding (emulator starved); could not run" >&2
      exit 2
    fi
    echo "run-on-device: screenshot does not show the probe's colour" >&2
    exit 1
  fi
fi
[ "$probe" -eq 2 ] || [ "$control" -eq 2 ] && exit 2
[ "$probe" -eq 0 ] && [ "$control" -eq 1 ]
