#!/data/data/com.termux/files/usr/bin/bash
# Build a signed debug APK for RustMC Launcher using Termux's aapt2/d8/apksigner.
set -euo pipefail
cd "$(dirname "$0")"

OUT=build/apk
LIBSO=target/release/librml_ui.so
PKG=dev.rustmc.launcher

echo "[1/6] cargo build (android cdylib)"
cargo build -p rml-android --release

echo "[2/6] javac"
rm -rf "$OUT"
mkdir -p "$OUT/classes" "$OUT/dex"
javac --release 11 -d "$OUT/classes" \
    -classpath android.jar \
    android/java/dev/rustmc/launcher/MainActivity.java

echo "[3/6] d8"
d8 --min-api 26 --lib android.jar --output "$OUT/dex" \
    "$OUT"/classes/dev/rustmc/launcher/*.class

echo "[4/6] aapt2 link"
aapt2 link -I android.jar \
    --manifest android/AndroidManifest.xml \
    --min-sdk-version 26 --target-sdk-version 35 \
    -o "$OUT/base.apk"

echo "[5/6] package + zipalign"
python3 - "$OUT/base.apk" "$OUT/dex/classes.dex" "$LIBSO" "$OUT/unsigned.apk" <<'PY'
import sys, zipfile
base, dex, so, out = sys.argv[1:5]
with zipfile.ZipFile(base) as zin, zipfile.ZipFile(out, "w") as zout:
    for item in zin.infolist():
        zout.writestr(item, zin.read(item.filename))
    zout.write(dex, "classes.dex", compress_type=zipfile.ZIP_DEFLATED)
    zout.write(so, "lib/arm64-v8a/librml_ui.so", compress_type=zipfile.ZIP_STORED)
PY
zipalign -f -p 4 "$OUT/unsigned.apk" "$OUT/aligned.apk"

echo "[6/6] sign"
if [ ! -f debug.keystore ]; then
    keytool -genkeypair -keystore debug.keystore -alias androiddebugkey \
        -storepass android -keypass android -keyalg RSA -keysize 2048 \
        -validity 10000 -dname "CN=Android Debug,O=Android,C=US" >/dev/null 2>&1
fi
apksigner sign --ks debug.keystore --ks-pass pass:android --key-pass pass:android \
    --out RustMC-Launcher.apk "$OUT/aligned.apk"
apksigner verify --print-certs RustMC-Launcher.apk | head -3

echo
echo "APK: $(pwd)/RustMC-Launcher.apk"
ls -la RustMC-Launcher.apk
