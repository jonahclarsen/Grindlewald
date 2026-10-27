#!/usr/bin/env bash
set -euo pipefail
PKG=com.jonahclarsen.grindlewald
PORT=$(cat android/ci-update-port.txt)
python3 -m http.server "$PORT" --bind 127.0.0.1 > /tmp/update-server.log 2>&1 &
SERVER_PID=$!
cleanup() {
  local cleanup_result=$?
  trap - EXIT
  adb logcat -d > android-log.txt || true
  adb exec-out screencap -p > android-screen.png || true
  if [ "$cleanup_result" != 0 ]; then
    adb root || true
    timeout 10 adb wait-for-device || true
    adb pull /data/anr android-anr || true
  fi
  kill "$SERVER_PID" || true
  exit "$cleanup_result"
}
trap cleanup EXIT
adb reverse "tcp:$PORT" "tcp:$PORT"
sleep 25
adb logcat -c
adb install grindlewald-debug.apk
adb shell pm grant "$PKG" android.permission.BLUETOOTH_SCAN
adb shell pm grant "$PKG" android.permission.BLUETOOTH_CONNECT
adb shell am start -n "$PKG/.MainActivity"
sleep 12
timeout 90 node scripts/android-webview.mjs prepare
adb shell am force-stop "$PKG"
adb shell run-as "$PKG" mkdir -p files
printf retained | adb shell run-as "$PKG" tee files/update-retention.txt
adb shell run-as "$PKG" touch files/updater-ci-selftest
adb shell appops set "$PKG" REQUEST_INSTALL_PACKAGES allow
BEFORE=$(adb shell pm path "$PKG")
adb logcat -c
adb shell am start -n "$PKG/.MainActivity"
for attempt in $(seq 1 60); do
  CODE=$(adb shell dumpsys package "$PKG" | sed -n 's/.*versionCode=\([0-9]*\).*/\1/p' | head -1)
  if [ "$CODE" = 1001 ]; then
    test "$(adb shell pm path "$PKG")" != "$BEFORE"
    adb shell run-as "$PKG" cat files/update-retention.txt | grep -E retained
    adb shell am start -n "$PKG/.MainActivity"
    timeout 90 node scripts/android-webview.mjs verify
    if adb logcat -d | grep -E 'FATAL EXCEPTION|UPDATE_E2E_FAIL'; then exit 1; fi
    echo 'PASS: actual downloaded APK upgraded 0.1.0 to 0.1.1 and retained data'
    exit 0
  fi
  sleep 2
done
exit 1
