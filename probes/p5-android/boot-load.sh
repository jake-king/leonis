#!/usr/bin/env bash
# Measures emulator CPU load from a cold boot, to test the hypothesis that "System UI isn't
# responding" is caused by CPU starvation during the first minutes after boot (docs/probes/p5.md).
#
# usage: boot-load.sh AVD OUTDIR [BOOTS] [CATCHES]
# For each cold boot (up to BOOTS, default 6; stops after CATCHES boots with the dialog, default 2):
# every 2 s for 180 s after adb connects, one line in OUTDIR/boot-N.tsv:
#   seconds-since-connect  boot_completed  cpu-busy-%  load-1m  focus  top-3-processes
# (top-3 %CPU is per core: 100 = one full core of the emulator's 4)
# On the first sample with a "Not Responding" window: OUTDIR/boot-N-cpuinfo.txt (dumpsys cpuinfo)
# and OUTDIR/boot-N-anr/ (the ANR traces from /data/anr, which needs adb root: AOSP images allow it).
# exit: 0 measured, 2 could not run
set -euo pipefail

avd=${1:?usage: boot-load.sh AVD OUTDIR [BOOTS] [CATCHES]}
out=${2:?usage: boot-load.sh AVD OUTDIR [BOOTS] [CATCHES]}
boots=${3:-6}
catches=${4:-2}
sdk=${ANDROID_HOME:-$HOME/Android/Sdk}
adb=$sdk/platform-tools/adb
emulator=$sdk/emulator/emulator
[ -x "$adb" ] && [ -x "$emulator" ] || { echo "boot-load: SDK tools not found under $sdk" >&2; exit 2; }
mkdir -p "$out"

# One sample: everything read in a single adb shell call so samples stay ~2 s apart.
sample() {
  "$adb" shell '
    head -1 /proc/stat
    cat /proc/loadavg
    getprop sys.boot_completed
    dumpsys window | grep -o "mCurrentFocus=.*" | head -1
    top -b -n 2 -d 1 -q -s 1 -o %CPU,NAME 2>/dev/null | awk "NF==0{b=1;next} b&&n<3{print;n++}" \
      | tr -s " " | tr "\n" ";"
  ' 2>/dev/null | tr -d '\r'
}

caught=0
for n in $(seq 1 "$boots"); do
  "$adb" emu kill >/dev/null 2>&1 || true
  sleep 5
  "$emulator" -avd "$avd" -no-window -gpu swiftshader_indirect -no-snapshot -no-audio -no-boot-anim \
    -memory 3072 > "$out/boot-$n-emulator.log" 2>&1 &
  timeout 240 "$adb" wait-for-device
  "$adb" root >/dev/null 2>&1 || true
  timeout 60 "$adb" wait-for-device
  tsv=$out/boot-$n.tsv
  printf 't\tboot_completed\tcpu_busy_pct\tload1\tfocus\ttop3\n' > "$tsv"
  prev_total=0 prev_idle=0 seen=no
  start=$(date +%s)
  while [ $(( $(date +%s) - start )) -lt 180 ]; do
    mapfile -t s < <(sample)
    # /proc/stat "cpu user nice system idle iowait irq softirq steal ..."
    read -r _ user nice system idle iowait irq softirq steal _ <<<"${s[0]:-cpu 0 0 0 0 0 0 0 0}"
    total=$((user + nice + system + idle + iowait + irq + softirq + steal))
    idle_all=$((idle + iowait))
    busy=NA
    if [ "$prev_total" -gt 0 ] && [ "$total" -gt "$prev_total" ]; then
      busy=$(( 100 * ( (total - prev_total) - (idle_all - prev_idle) ) / (total - prev_total) ))
    fi
    prev_total=$total prev_idle=$idle_all
    focus=${s[3]:-}
    printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$(( $(date +%s) - start ))" "${s[2]:-}" "$busy" \
      "$(cut -d' ' -f1 <<<"${s[1]:-}")" "${focus#mCurrentFocus=}" "${s[4]:-}" >> "$tsv"
    if [ "$seen" = no ] && [[ "$focus" == *"Not Responding"* ]]; then
      seen=yes
      caught=$((caught + 1))
      "$adb" shell dumpsys cpuinfo > "$out/boot-$n-cpuinfo.txt" 2>&1 || true
      mkdir -p "$out/boot-$n-anr"
      "$adb" pull /data/anr/. "$out/boot-$n-anr/" >/dev/null 2>&1 || true
    fi
    sleep 2
  done
  echo "boot $n: dialog=$seen samples=$(( $(wc -l < "$tsv") - 1 ))"
  [ "$caught" -ge "$catches" ] && break
done
"$adb" emu kill >/dev/null 2>&1 || true
echo "boot-load: $caught of $n cold boots showed the dialog; data in $out"
