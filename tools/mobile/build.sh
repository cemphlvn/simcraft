#!/usr/bin/env bash
# Builds sim-mobile for a phone (docs/architecture.md, Platforms and builds).
#
#   tools/mobile/build.sh ios sim       build for the iOS simulator, install it on a booted (or new) iPhone, launch it,
#                                        and save a screenshot to target/mobile/ios/screen.png
#   tools/mobile/build.sh ios device    build for an iPhone (development signing)
#   tools/mobile/build.sh ios phone     build, install and launch on your paired iPhone over Wi-Fi (no cable): the first
#                                        one `xcrun devicectl list devices` shows available, or SIMCRAFT_PHONE=<identifier>
#   tools/mobile/build.sh ios archive   an App Store archive: target/mobile/ios/Simcraft.xcarchive (upload it from
#                                        Xcode's Organizer, or `xcodebuild -exportArchive`)
#   tools/mobile/build.sh android apk   a debug APK for a device or emulator
#   tools/mobile/build.sh android aab   a release App Bundle for Play (sign it with your upload key)
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
out="$root/target/mobile"
platform="${1:-}"
what="${2:-}"

die() { echo "build.sh: $*" >&2; exit 2; }

ios() {
    command -v xcodegen >/dev/null || die "xcodegen is missing: brew install xcodegen"
    local target sdk
    case "$what" in
        sim) target=aarch64-apple-ios-sim; sdk=iphonesimulator ;;
        device|phone|archive) target=aarch64-apple-ios; sdk=iphoneos ;;
        *) die "ios: sim | device | phone | archive" ;;
    esac
    rustup target list --installed | grep -qx "$target" || die "missing Rust target: rustup target add $target"
    cargo build --manifest-path "$root/Cargo.toml" -p sim-mobile --lib --release --target "$target"
    (cd "$root/tools/mobile/ios" && xcodegen generate --quiet)
    local proj="$root/tools/mobile/ios/Simcraft.xcodeproj" derived="$out/ios/derived"
    case "$what" in
        sim)
            xcodebuild -project "$proj" -scheme Simcraft -configuration Release -sdk "$sdk" \
                -destination 'generic/platform=iOS Simulator' -derivedDataPath "$derived" CODE_SIGNING_ALLOWED=NO -quiet build
            local app="$derived/Build/Products/Release-iphonesimulator/Simcraft.app"
            local udid
            udid="$(xcrun simctl list devices booted | grep -oE '[0-9A-F-]{36}' | head -1 || true)"
            if [ -z "$udid" ]; then
                udid="$(xcrun simctl list devices available | grep -E 'iPhone' | grep -oE '[0-9A-F-]{36}' | head -1)"
                [ -n "$udid" ] || die "no iPhone simulator: create one in Xcode"
                xcrun simctl boot "$udid"
            fi
            xcrun simctl install "$udid" "$app"
            xcrun simctl launch "$udid" dev.simcraft.mobile
            sleep 3
            mkdir -p "$out/ios"
            xcrun simctl io "$udid" screenshot "$out/ios/screen.png" >/dev/null
            echo "running on simulator $udid; screenshot: $out/ios/screen.png"
            ;;
        device|phone)
            xcodebuild -project "$proj" -scheme Simcraft -configuration Release -sdk "$sdk" \
                -destination 'generic/platform=iOS' -derivedDataPath "$derived" -allowProvisioningUpdates -quiet build
            local app="$derived/Build/Products/Release-iphoneos/Simcraft.app"
            echo "built: $app"
            if [ "$what" = phone ]; then
                local phone="${SIMCRAFT_PHONE:-}"
                if [ -z "$phone" ]; then
                    phone="$(xcrun devicectl list devices 2>/dev/null | grep -E 'available \(paired\)|connected' | grep -m1 iPhone | grep -oE '[0-9A-F]{8}-[0-9A-F-]{27}' || true)"
                fi
                [ -n "$phone" ] || die "no paired iPhone available: same Wi-Fi as this Mac, unlocked, paired once in Xcode (Window > Devices)"
                xcrun devicectl device install app --device "$phone" "$app" >/dev/null
                xcrun devicectl device process launch --device "$phone" dev.simcraft.mobile >/dev/null
                echo "running on iPhone $phone"
            fi
            ;;
        archive)
            xcodebuild -project "$proj" -scheme Simcraft -configuration Release -sdk "$sdk" \
                -destination 'generic/platform=iOS' -archivePath "$out/ios/Simcraft.xcarchive" -allowProvisioningUpdates -quiet archive
            echo "archive: $out/ios/Simcraft.xcarchive"
            ;;
    esac
}

android() {
    case "$what" in apk|aab) ;; *) die "android: apk | aab" ;; esac
    export ANDROID_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
    export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/28.2.13676358}"
    if [ -z "${JAVA_HOME:-}" ] && [ -d /opt/homebrew/opt/openjdk@17 ]; then
        export JAVA_HOME=/opt/homebrew/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home
    fi
    [ -d "$ANDROID_NDK_HOME" ] || die "no NDK at $ANDROID_NDK_HOME: sdkmanager 'ndk;28.2.13676358'"
    command -v cargo-ndk >/dev/null || die "cargo-ndk is missing: cargo install cargo-ndk"
    command -v gradle >/dev/null || die "gradle is missing: brew install gradle"
    rustup target list --installed | grep -qx aarch64-linux-android || die "missing Rust target: rustup target add aarch64-linux-android"
    # API 26 is the app's minSdk; NDK r28 aligns every segment to 16 KB pages, as Play requires.
    cargo ndk --manifest-path "$root/Cargo.toml" -t arm64-v8a -P 26 -o "$out/android/jniLibs" build -p sim-mobile --lib --release
    local proj="$root/tools/mobile/android"
    echo "sdk.dir=$ANDROID_HOME" > "$proj/local.properties"
    local adb="$ANDROID_HOME/platform-tools/adb"
    case "$what" in
        apk)
            gradle -p "$proj" --quiet assembleDebug
            local apk="$proj/app/build/outputs/apk/debug/app-debug.apk"
            if ! "$adb" get-state >/dev/null 2>&1; then
                local avd=simcraft_api36
                if ! "$ANDROID_HOME/emulator/emulator" -list-avds | grep -qx "$avd"; then
                    local avdmanager
                    # The SDK's own copy (sdkmanager 'cmdline-tools;latest'): it finds the system images next to it.
                    avdmanager="$ANDROID_HOME/cmdline-tools/latest/bin/avdmanager"
                    [ -x "$avdmanager" ] || die "no avdmanager in the SDK: sdkmanager 'cmdline-tools;latest'"
                    echo no | "$avdmanager" create avd -n "$avd" -k 'system-images;android-36;google_apis;arm64-v8a' -d pixel_8 >/dev/null
                fi
                "$ANDROID_HOME/emulator/emulator" -avd "$avd" -no-window -no-audio -no-boot-anim >/dev/null 2>&1 &
                "$adb" wait-for-device
                until [ "$("$adb" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = 1 ]; do sleep 2; done
            fi
            "$adb" install -r "$apk" >/dev/null
            "$adb" shell am start -n dev.simcraft.mobile/android.app.NativeActivity >/dev/null
            sleep 4
            mkdir -p "$out/android"
            "$adb" exec-out screencap -p > "$out/android/screen.png"
            echo "running on $("$adb" get-serialno); screenshot: $out/android/screen.png"
            ;;
        aab)
            gradle -p "$proj" --quiet bundleRelease
            local aab="$proj/app/build/outputs/bundle/release/app-release.aab"
            mkdir -p "$out/android"
            cp "$aab" "$out/android/simcraft.aab"
            # Play takes a bundle signed with your upload key: SIMCRAFT_UPLOAD_KEYSTORE (+ _ALIAS, _PASS) signs it.
            if [ -n "${SIMCRAFT_UPLOAD_KEYSTORE:-}" ]; then
                "$JAVA_HOME/bin/jarsigner" -keystore "$SIMCRAFT_UPLOAD_KEYSTORE" -storepass "${SIMCRAFT_UPLOAD_PASS:?}" \
                    "$out/android/simcraft.aab" "${SIMCRAFT_UPLOAD_ALIAS:?}" >/dev/null
                echo "signed bundle: $out/android/simcraft.aab"
            else
                echo "unsigned bundle: $out/android/simcraft.aab (set SIMCRAFT_UPLOAD_KEYSTORE to sign it for Play)"
            fi
            ;;
    esac
}

case "$platform" in
    ios) ios ;;
    android) android ;;
    *) die "usage: build.sh ios (sim|device|archive) | android (apk|aab)" ;;
esac
