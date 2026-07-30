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
| Rust target | `aarch64-linux-android` | rustup |
| `cargo-ndk` | 4.1.2 | `~/.cargo/bin` |

About 2.7 GB of SDK plus 200 MB of JDK.

**JDK 21, not 26.** The system JDK is 26 and the Android Gradle Plugin does not
run on it. 21 is the current LTS the Android tooling targets, so it is installed
alongside rather than replacing anything — `JAVA_HOME` decides which one is used
and the system one is left as it was.

**API 26 (Android 8) as the floor, `arm64-v8a` only.** 26 covers essentially
every device still running, and one ABI keeps the APK small and the build short.
Adding `armeabi-v7a` and `x86_64` later is one flag; nothing in the code assumes
a single architecture.

## Building the native library

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
