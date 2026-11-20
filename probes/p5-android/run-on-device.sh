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

"$adb" install -r "$apk" >/dev/null

# Runs the probe once with the given mode; prints its result line; returns the probe's exit code
# (2 if no verdict appeared within the timeout).
run() {
  local mode=$1 save=${2:-}
  "$adb" logcat -c
  "$adb" shell setprop debug.leonis.p5 "${mode:-\'\'}"
  "$adb" shell am start -S -n org.leonis.p5/org.libsdl.app.SDLActivity >/dev/null
  if [ -n "$save" ]; then
    sleep 1.2
    "$adb" exec-out screencap -p > "$save"
  fi
  local log=""
  for _ in $(seq 1 45); do
    log=$("$adb" logcat -d -s leonis:I)
    grep -q 'p5 exit=' <<<"$log" && break
    sleep 2
  done
  grep -E 'p5 (setup|result)' <<<"$log" | sed -E 's/^.*leonis *: //'
  local code
  code=$(grep -oE 'p5 exit=[0-9]+' <<<"$log" | tail -1 | cut -d= -f2)
  if [ -z "$code" ]; then
    echo "run-on-device: no verdict from mode '${mode}' in 90 s; crash lines:" >&2
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
[ "$probe" -eq 2 ] || [ "$control" -eq 2 ] && exit 2
[ "$probe" -eq 0 ] && [ "$control" -eq 1 ]
