#!/usr/bin/env bash
# Builds the P5 probe into a signed debug APK with the SDK's own tools: no Gradle, no Java or Kotlin
# of ours. SDL's Java activity comes from SDL's source (the sdl3-src crate cargo already downloaded).
#
# usage: build-apk.sh [x86_64|arm64-v8a]     (default x86_64, the emulator ABI)
# env:   ANDROID_HOME (default ~/Android/Sdk), NDK_VERSION, BUILD_TOOLS, ANDROID_API
# out:   build/<abi>/p5.apk
# exit:  0 built, 1 a step failed, 2 a required tool or input is missing
set -euo pipefail
cd "$(dirname "$0")"

ABI=${1:-x86_64}
case "$ABI" in
  x86_64) TARGET=x86_64-linux-android ;;
  arm64-v8a) TARGET=aarch64-linux-android ;;
  *) echo "build-apk: unknown ABI $ABI" >&2; exit 2 ;;
esac
MIN_API=29
SDK=${ANDROID_HOME:-$HOME/Android/Sdk}
NDK=$SDK/ndk/${NDK_VERSION:-29.0.14206865}
BT=$SDK/build-tools/${BUILD_TOOLS:-36.1.0}
JAR=$SDK/platforms/android-${ANDROID_API:-36}/android.jar
TC=$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin
for need in "$NDK" "$BT/aapt2" "$BT/d8" "$BT/zipalign" "$BT/apksigner" "$JAR" "$TC/clang"; do
  [ -e "$need" ] || { echo "build-apk: missing $need" >&2; exit 2; }
done
for tool in cargo javac keytool zip; do
  command -v "$tool" >/dev/null || { echo "build-apk: missing $tool on PATH" >&2; exit 2; }
done

# --- Rust: libmain.so, and libSDL3.so built by sdl3-sys through the NDK's CMake toolchain ---------
target_env=$(echo "$TARGET" | tr 'a-z-' 'A-Z_')
clang="$TC/${TARGET}${MIN_API}-clang"
export "CARGO_TARGET_${target_env}_LINKER=$clang"
export "CC_${TARGET//-/_}=$clang" "CXX_${TARGET//-/_}=${clang}++" "AR_${TARGET//-/_}=$TC/llvm-ar"
export ANDROID_NDK=$NDK ANDROID_NDK_ROOT=$NDK
export CMAKE_TOOLCHAIN_FILE=$NDK/build/cmake/android.toolchain.cmake
export ANDROID_PLATFORM=android-$MIN_API
cargo build --locked --release --target "$TARGET"

out=build/$ABI
rm -rf "$out" && mkdir -p "$out/stage/lib/$ABI" "$out/classes"
cp "target/$TARGET/release/libmain.so" "$out/stage/lib/$ABI/"
mapfile -t sdl < <(find "target/$TARGET/release/build" -path '*sdl3-sys-*/out/lib/libSDL3.so' -newer Cargo.toml)
[ "${#sdl[@]}" -eq 1 ] || { echo "build-apk: expected one libSDL3.so, found ${#sdl[@]}: ${sdl[*]:-none}" >&2; exit 1; }
cp "${sdl[0]}" "$out/stage/lib/$ABI/"

# --- Java: SDL's activity, compiled from SDL's source ------------------------------------------
src=$(find "${CARGO_HOME:-$HOME/.cargo}/registry/src" -maxdepth 1 -mindepth 1 -type d -print0 \
  | xargs -0 -I{} find {} -maxdepth 1 -type d -name 'sdl3-src-*' | sort -V | tail -1)
java=$src/SDL/android-project/app/src/main/java
[ -d "$java/org/libsdl/app" ] || { echo "build-apk: SDL Java sources not found under $src" >&2; exit 2; }
javac -nowarn -Xlint:-options --release 17 -cp "$JAR" -d "$out/classes" "$java"/org/libsdl/app/*.java
mapfile -t classes < <(find "$out/classes" -name '*.class')
"$BT/d8" --release --min-api "$MIN_API" --lib "$JAR" --output "$out/stage" "${classes[@]}"

# --- Manifest, resources, package, align, sign -------------------------------------------------
cat > "$out/AndroidManifest.xml" <<EOF
<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="org.leonis.p5" android:versionCode="1" android:versionName="0.0.0">
  <uses-feature android:glEsVersion="0x00020000" />
  <application android:label="leonis P5" android:hasCode="true" android:extractNativeLibs="true"
      android:hardwareAccelerated="true" android:theme="@android:style/Theme.DeviceDefault.NoActionBar.Fullscreen">
    <activity android:name="org.libsdl.app.SDLActivity" android:exported="true"
        android:configChanges="layoutDirection|locale|orientation|uiMode|screenLayout|screenSize|smallestScreenSize|keyboard|keyboardHidden|navigation"
        android:preferMinimalPostProcessing="true">
      <intent-filter>
        <action android:name="android.intent.action.MAIN" />
        <category android:name="android.intent.category.LAUNCHER" />
      </intent-filter>
    </activity>
  </application>
</manifest>
EOF
"$BT/aapt2" link -o "$out/unsigned.apk" -I "$JAR" --manifest "$out/AndroidManifest.xml" \
  --min-sdk-version "$MIN_API" --target-sdk-version "${ANDROID_API:-36}"
(cd "$out/stage" && zip -q -r ../unsigned.apk classes.dex lib)
"$BT/zipalign" -P 16 -f 4 "$out/unsigned.apk" "$out/aligned.apk"

# A throwaway debug key, made once per checkout; its password is not a secret (Android's own
# debug-key convention) but goes through the environment, never argv.
ks=build/debug.keystore
export LEONIS_DEBUG_KS_PASS=android
if [ ! -f "$ks" ]; then
  keytool -genkeypair -keystore "$ks" -storepass:env LEONIS_DEBUG_KS_PASS -alias debug \
    -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=leonis debug" >/dev/null 2>&1
fi
"$BT/apksigner" sign --ks "$ks" --ks-pass env:LEONIS_DEBUG_KS_PASS --out "$out/p5.apk" "$out/aligned.apk"
"$BT/apksigner" verify "$out/p5.apk"
echo "build-apk: $out/p5.apk ($(du -h "$out/p5.apk" | cut -f1))"
