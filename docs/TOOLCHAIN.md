# Android toolchain

Everything lives in `$HOME`; nothing here needs root, which matters because this
machine has no passwordless `sudo`. Set these three and the rest follows:

```fish
set -gx JAVA_HOME       ~/opt/jdk21
set -gx ANDROID_HOME    ~/Android/Sdk
set -gx ANDROID_NDK_HOME ~/Android/Sdk/ndk/28.2.13676358
set -gx PATH $JAVA_HOME/bin $ANDROID_HOME/cmdline-tools/latest/bin $ANDROID_HOME/platform-tools $PATH
```

| Piece | Version | Where |
|---|---|---|
| JDK | Temurin 21.0.12 LTS | `~/opt/jdk21` |
| SDK cmdline-tools | 13114758 | `~/Android/Sdk/cmdline-tools/latest` |
| Platform | android-36 | `~/Android/Sdk/platforms` |
| Build-tools | 36.0.0 | `~/Android/Sdk/build-tools` |
| NDK | 28.2.13676358 (r28c) | `~/Android/Sdk/ndk` |
| Emulator | 37.1.11 | `~/Android/Sdk/emulator` |
| System image | android-36 `default;x86_64` | `~/Android/Sdk/system-images` |
| Gradle | 8.14.5 | `~/opt/gradle-8.14.5`, then the wrapper |
| Rust targets | `aarch64-linux-android`, `x86_64-linux-android` | rustup |
| `cargo-ndk` | 4.1.2 | `~/.cargo/bin` |

About 6 GB in total: SDK, NDK, emulator and system image, plus the JDK and
Gradle. Only the wrapper is committed — `android/gradlew` downloads Gradle
itself, so the copy in `~/opt` is just what generated it.

**JDK 21, not 26.** The system JDK is 26 and the Android Gradle Plugin does not
run on it. 21 is the current LTS the Android tooling targets, so it is installed
alongside rather than replacing anything — `JAVA_HOME` decides which one is used
and the system one is left as it was.

**AGP 8.13.2, and the AndroidX versions pinned to match it.** The newest
AndroidX releases require AGP 9 and `compileSdk` 37; moving to those would mean
a new Gradle major, a new platform and a new set of plugin APIs, all to gain
nothing this app uses. The versions in `gradle/libs.versions.toml` are the last
ones that build against `compileSdk` 36 — chosen deliberately, not left behind.

**API 26 (Android 8) as the floor.** 26 covers essentially every device still
running.

**Two ABIs: `arm64-v8a` and `x86_64`.** The first is every phone worth
targeting. The second exists so the app can run on an emulator — without it the
only way to test the APK is by hand on a device, which is to say not routinely.
Half a megabyte for a build that can be verified automatically is not a trade
worth thinking about. `armeabi-v7a` is one more entry in the same list;
nothing in the code assumes an architecture.

## Building the app

```console
$ cd android
$ ./gradlew :app:assembleDebug
```

`:app:cargoNdk` runs first and cross-compiles the library into
`app/build/rustJniLibs/<abi>/`, which is on `jniLibs.srcDirs`. Its inputs are
declared, so a rebuild that only touched Kotlin skips it entirely.

Always `--release` for the Rust side, even in a debug APK: a debug build of the
resampler is not slow in the ordinary sense, it is unusable.

## The release build

```console
$ cd android
$ ./gradlew :app:assembleRelease
```

Shrunk by R8 to about 2.4 MB, against 12.3 MB for the debug build. Signing is
optional: `build.gradle.kts` reads `keystore.properties` if it is there, and
builds unsigned if it is not. **That file is gitignored and must stay that way** —
a signing key in version control is a key anyone who clones the repository can
publish updates with.

```properties
storeFile=/path/outside/the/repo/release.jks
storePassword=…
keyAlias=…
keyPassword=…
```

Shrinking is where a JNI app usually breaks. Nothing in the Kotlin calls
`Java_org_loopslcr_Native_analyze`; the linker does, at run time, by matching a
symbol in the shared library against a class and method name. R8 cannot see
that, so without keep rules the build shrinks perfectly, installs perfectly, and
throws on the first file opened. The rules are in `app/proguard-rules.pro` —
and in `app/proguard-rules-test.pro`, because the instrumentation APK is shrunk
in a separate pass that does not inherit them.

## Testing on a device or emulator

```console
$ ~/Android/Sdk/emulator/emulator -avd loopslcr -no-window -no-audio -gpu swiftshader_indirect &
$ cd android && ./gradlew :app:connectedDebugAndroidTest
```

The AVD was made with:

```console
$ avdmanager create avd -n loopslcr -k "system-images;android-36;default;x86_64" -d pixel_6
```

`/dev/kvm` on this machine is world-writable, so the emulator runs accelerated
without adding anyone to the `kvm` group.

`app/src/androidTest` runs the whole engine on the device — analyse, plan,
process, peaks, reproducibility, the panic guard — and renders the cutter
screen, writing a screenshot to the app's `filesDir`:

```console
$ adb exec-out run-as org.loopslcr.app cat files/cutter.png > cutter.png
```

To run the same suite against the **shrunk** build, which is the only place a
wrong keep rule shows up:

```console
$ ./gradlew -PtestRelease :app:connectedAndroidTest
```

## Building the native library on its own

```console
$ cargo ndk -t arm64-v8a --platform 26 build --release -p loopslcr-jni
$ file target/aarch64-linux-android/release/libloopslcr_jni.so
… ELF 64-bit LSB shared object, ARM aarch64, for Android 26, built by NDK r28c
```

## Testing the bridge without a device

`cargo test -p loopslcr-jni` builds the host `cdylib`, compiles
`tests/java` with `javac`, and runs it in a real JVM with the library loaded.
That covers the direct `ByteBuffer` transfer, the returned arrays, the error
mapping, and the panic guard — everything except what needs Android itself.

The test skips with a message when no JDK is found, so the CLI still builds and
tests on a machine without any of this.
