## Fuss

A minimal, fast Android file manager: a Rust filesystem engine behind a small
JNI surface, with a Jetpack Compose (Material 3) interface.

### Install

1. Pick the APK for your device:
   * `fuss-release-arm64-v8a.apk` — almost every phone and tablet since 2019
     (this is the primary target).
   * `fuss-release-armeabi-v7a.apk` — older 32-bit ARM devices.
   * `fuss-release-universal.apk` — if you are unsure.
2. Allow installing unknown apps for your browser/files app when prompted.
3. Verify the download against `SHA256SUMS.txt` if you like:
   `sha256sum -c SHA256SUMS.txt`

`fuss-debug-*.apk` builds are included for testing; they are slower and larger.

### Notes

* Android 11 (API 30) or newer.
* No internet permission: the app cannot talk to the network at all.
* Parts of shared storage that Android hides from apps are opened through
  **Add folder** on the home screen (SAF), one folder at a time.
* Release APKs are signed with the project's test key unless the maintainer
  supplied a distribution key to the build. Check the signing certificate before
  treating a build as official.
