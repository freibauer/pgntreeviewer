#!/usr/bin/env bash
# Builds android/PgnTreeViewer.apk (Android 8.0+, arm64) without Gradle:
#   cargo-ndk  -> lib/arm64-v8a/libpgntreeviewer.so   (Rust/egui app)
#   javac + d8 -> classes.dex                        (MainActivity.java)
#   aapt2      -> manifest + icon resources
#   zipalign + apksigner
#
# One-time setup (see README "Android"):
#   - JDK 17, Android SDK with platforms;android-34, build-tools;34.0.0 and an NDK
#   - rustup target add aarch64-linux-android && cargo install cargo-ndk
#
# Paths can be overridden with environment variables:
#   ANDROID_HOME   (default ~/android/sdk)
#   JAVA_HOME      (default ~/android/jdk17)
#   ANDROID_NDK_HOME (default: newest NDK in $ANDROID_HOME/ndk)
#   APK_KEYSTORE   (default ~/android/keys/pgntreeviewer.jks, created if missing)
#   APK_KEYPASS    (default "pgntreeviewer")
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

export ANDROID_HOME="${ANDROID_HOME:-$HOME/android/sdk}"
export JAVA_HOME="${JAVA_HOME:-$HOME/android/jdk17}"
export PATH="$JAVA_HOME/bin:$PATH"
if [[ -z "${ANDROID_NDK_HOME:-}" ]]; then
    ANDROID_NDK_HOME="$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | tail -1)"
fi
export ANDROID_NDK_HOME

PLATFORM="$ANDROID_HOME/platforms/android-34/android.jar"
BT="$ANDROID_HOME/build-tools/34.0.0"
KEYSTORE="${APK_KEYSTORE:-$HOME/android/keys/pgntreeviewer.jks}"
KEYPASS="${APK_KEYPASS:-pgntreeviewer}"
VERSION_NAME="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
VERSION_CODE="${APK_VERSION_CODE:-1}"

for f in "$PLATFORM" "$BT/aapt2" "$BT/d8" "$BT/zipalign" "$BT/apksigner" "$JAVA_HOME/bin/javac"; do
    [[ -e "$f" ]] || { echo "Error: $f not found (see one-time setup at the top of this script)" >&2; exit 1; }
done
[[ -d "$ANDROID_NDK_HOME" ]] || { echo "Error: no Android NDK found" >&2; exit 1; }

OUT="android/build"
rm -rf "$OUT/classes" "$OUT/dex" "$OUT/apk"
mkdir -p "$OUT/classes" "$OUT/dex" "$OUT/apk"

echo "==> Rust library (arm64-v8a)"
cargo ndk -t arm64-v8a -P 26 -o "$OUT/jniLibs" build --release --lib

echo "==> Java activity"
javac -source 8 -target 8 -Xlint:-options -classpath "$PLATFORM" -d "$OUT/classes" \
    $(find android/java -name '*.java')
"$BT/d8" --release --min-api 26 --lib "$PLATFORM" --output "$OUT/dex" $(find "$OUT/classes" -name '*.class')

echo "==> Resources + manifest"
"$BT/aapt2" compile --dir android/res -o "$OUT/res.zip"
"$BT/aapt2" link -o "$OUT/apk/base.apk" -I "$PLATFORM" \
    --manifest android/AndroidManifest.xml \
    --min-sdk-version 26 --target-sdk-version 34 \
    --version-code "$VERSION_CODE" --version-name "$VERSION_NAME" \
    "$OUT/res.zip"

echo "==> Packaging"
python3 - "$OUT" <<'EOF'
import sys, zipfile, shutil
out = sys.argv[1]
shutil.copy(f"{out}/apk/base.apk", f"{out}/apk/unaligned.apk")
with zipfile.ZipFile(f"{out}/apk/unaligned.apk", "a") as z:
    z.write(f"{out}/dex/classes.dex", "classes.dex", compress_type=zipfile.ZIP_DEFLATED)
    z.write(f"{out}/jniLibs/arm64-v8a/libpgntreeviewer.so", "lib/arm64-v8a/libpgntreeviewer.so",
            compress_type=zipfile.ZIP_DEFLATED)
EOF
"$BT/zipalign" -p -f 4 "$OUT/apk/unaligned.apk" "$OUT/apk/aligned.apk"

echo "==> Signing"
if [[ ! -f "$KEYSTORE" ]]; then
    echo "    creating signing key $KEYSTORE (keep it: updates must be signed with the same key)"
    mkdir -p "$(dirname "$KEYSTORE")"
    keytool -genkeypair -keystore "$KEYSTORE" -alias pgntreeviewer -keyalg RSA -keysize 2048 \
        -validity 10000 -storepass "$KEYPASS" -keypass "$KEYPASS" -dname "CN=PGN Tree Viewer"
fi
"$BT/apksigner" sign --ks "$KEYSTORE" --ks-key-alias pgntreeviewer \
    --ks-pass "pass:$KEYPASS" --key-pass "pass:$KEYPASS" \
    --out android/PgnTreeViewer.apk "$OUT/apk/aligned.apk"
"$BT/apksigner" verify android/PgnTreeViewer.apk
rm -f android/PgnTreeViewer.apk.idsig
echo "==> Android build ready: $SCRIPT_DIR/android/PgnTreeViewer.apk"
