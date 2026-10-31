# Subsystem: `tidepool-mobile`

Builds, signs, verifies and installs Android and iOS apps. Every tool it
uses comes from `toolchain` (ADR-0002); every signature comes from the
platform's official signer (ADR-0007).

## Artifacts

| Purpose | Android | iOS equivalent |
| :- | :- | :- |
| Install on your own device | Debug APK (debug key) | Development-signed `.app` / `.ipa` (development certificate + development provisioning profile) |
| Direct distribution | Release APK | Ad hoc `.ipa` (registered devices) or Enterprise `.ipa` |
| Store submission | `.aab` (Play generates per-device APKs) | `.xcarchive` exported to an App Store `.ipa` (App Store Connect does app thinning) |
| Signing identity | Keystore (`.jks`/`.p12`) + key alias | Certificate + private key in the Keychain + provisioning profile + entitlements |
| Verification | `apksigner verify --verbose`, `zipalign -c -p 4`, `jarsigner -verify`, `bundletool validate` | `codesign --verify --deep --strict`; profile decoded with `security cms -D -i` |

## Contents

| Module | Provides |
| :- | :- |
| `project` | Detects Gradle projects (`settings.gradle(.kts)`, `gradlew`), Xcode projects (`.xcodeproj`, `.xcworkspace`), and Tidepool manifests (`tidepool-android.toml`, `tidepool-ios.toml`) |
| `android::gradle` | Runs the project's wrapper (`./gradlew :app:assembleRelease`, `:app:bundleRelease`) in trusted workspaces; parses output for errors; finds artifacts |
| `android::direct` | Gradle-free pipeline (below) |
| `android::sign` | Keystore creation (`keytool -genkeypair`), alias listing, APK and AAB signing and verification |
| `android::devices` | `adb devices -l`, install, launch, `logcat` streaming; AVD list/create/start via `avdmanager` and `emulator` |
| `ios::xcode` | `xcodebuild -list`, `archive`, `-exportArchive` with a generated `ExportOptions.plist` |
| `ios::direct` | Xcode-project-free Rust apps (below) |
| `ios::sign` | Identities (`security find-identity -v -p codesigning`), profile decoding and matching (bundle ID, team, certificates, devices, expiry), entitlements, `codesign` |
| `ios::devices` | Simulators (`xcrun simctl list/boot/install/launch`); physical devices through the tools the installed Xcode provides |
| `ios::remote` | Remote-Mac mode (ADR-0008): `rsync` the project over `ssh`, run the pipeline there, copy artifacts back |
| `inspect` | Opens APK, AAB and IPA as archives (`codec::zip`) and shows the manifest, ABIs, resources, signatures summary and sizes |

## Android: Gradle-free pipeline (`tidepool-android.toml`)

For Java/Kotlin apps without Gradle and for Rust `NativeActivity` apps.

1. `aapt2 compile` the resources; `aapt2 link` against the platform's
   `android.jar` (with `--proto-format` for bundles).
2. Compile JVM code (`javac`, or `kotlinc` if the user installs it) and dex
   it with `d8`.
3. Rust: `cargo build --release --target <android triple>` with the NDK's
   clang as linker (set through `CARGO_TARGET_<TRIPLE>_LINKER` on the
   child's environment); place `.so` files under `lib/<abi>/`.
4. **APK:** assemble the zip, `zipalign -p 4`, `apksigner sign`,
   `apksigner verify --verbose`.
5. **AAB:** assemble the base module zip (`manifest/AndroidManifest.xml`,
   `dex/`, `res/`, `lib/`, `assets/`, `root/`, `resources.pb`),
   `bundletool build-bundle --modules=base.zip`, `jarsigner`,
   `jarsigner -verify`. For device testing, `bundletool build-apks` with the
   same keystore, then `bundletool install-apks`.

Android's docs caution that the AAPT2 in build-tools may not support
`bundletool`. Confirm the right AAPT2 source when M10 starts and record it
in the descriptor.

## iOS: Xcode-project-free Rust apps (`tidepool-ios.toml`)

1. `cargo build --release --target aarch64-apple-ios` (and
   `aarch64-apple-ios-sim` for the simulator).
2. Assemble `<App>.app`: executable, `Info.plist` written by
   `codec::plist`, resources, icons (asset catalogs compiled with `actool`
   when present).
3. Embed the matching provisioning profile as `embedded.mobileprovision`;
   derive entitlements from it.
4. `codesign --force --sign <identity> --entitlements <file> <App>.app`;
   verify.
5. Zip `Payload/<App>.app` into `<App>.ipa` with `codec::zip`.

## Secret handling

- Keystore and key passwords are prompted per session and held in memory
  as `SecretArg` values; never stored in v1.
- They reach tools by stdin (`apksigner --ks-pass stdin`), a per-child
  environment variable (`apksigner --ks-pass env:NAME`, `jarsigner
  -storepass:env NAME`, `-keypass:env NAME`), or, for tools that accept only
  `pass:` or `file:` (`bundletool`), a 0600 file in a 0700 temp directory
  removed after the call, on every path.
- Signing identities stay in the Keychain; Tidepool refers to them by name
  or SHA-1 fingerprint and never exports keys.
- Keystores, `.p12`, `.p8`, provisioning profiles and `keystore.properties`
  are excluded from search indexes, AI context and logs.
- The UI explains Play App Signing (upload key vs app signing key) and
  reminds the user to back up keystores.

## Tests

- Fake tools reproduce `aapt2`, `d8`, `zipalign`, `apksigner`, `jarsigner`,
  `bundletool`, `adb`, `xcodebuild`, `codesign`, `security` and `simctl`
  output and exit codes.
- A test asserts that no constructed `Command` argv contains a secret.
- `TIDEPOOL_IT_ANDROID=1`: the Java and Rust samples produce a signed APK
  and AAB that pass every verifier, and install on an emulator.
- `TIDEPOOL_IT_IOS=1` (macOS): the samples produce development and App
  Store `.ipa` files that pass `codesign --verify --deep --strict`; the
  simulator build launches.
