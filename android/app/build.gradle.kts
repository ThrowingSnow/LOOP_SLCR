import java.util.Properties

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
}

/**
 * The ABIs the APK carries.
 *
 * `arm64-v8a` is every phone worth targeting. `x86_64` is there for one reason:
 * without it the app cannot run on an emulator, and an app that cannot run on an
 * emulator can only be tested by hand on a device. Half a megabyte to make the
 * instrumented tests possible is not a trade worth thinking about twice.
 */
val abis = listOf("arm64-v8a", "x86_64")

/// Where cargo-ndk drops `<abi>/libloopslcr_jni.so`.
val jniLibsDir = layout.buildDirectory.dir("rustJniLibs")

/**
 * Builds the native library.
 *
 * Always `--release`, even for a debug APK. A debug Rust build of a resampler
 * is not slow in the usual sense of the word — it is unusable, tens of times
 * slower, enough to turn a cut into a wait. Debugging the Kotlin does not need
 * a debug build of the DSP, and the DSP is tested on the host anyway.
 */
val cargoNdk = tasks.register<Exec>("cargoNdk") {
    group = "build"
    description = "Cross-compiles loopslcr-jni for ${abis.joinToString(", ")}"

    val workspace = rootProject.projectDir.parentFile
    workingDir = workspace

    // The sources that can change the library. Declared so Gradle can skip the
    // whole thing on a rebuild that only touched Kotlin — cargo would decide the
    // same in a second, but Gradle should not have to ask.
    inputs.files(fileTree(File(workspace, "crates")) { include("**/*.rs", "**/Cargo.toml") })
    inputs.file(File(workspace, "Cargo.lock"))
    outputs.dir(jniLibsDir)

    commandLine(
        buildList {
            add("cargo")
            add("ndk")
            abis.forEach { add("-t"); add(it) }
            add("--platform"); add("26")
            add("-o"); add(jniLibsDir.get().asFile.absolutePath)
            add("build")
            add("--release")
            add("-p"); add("loopslcr-jni")
        },
    )
}

android {
    namespace = "org.loopslcr.app"
    compileSdk = 36

    // Named so AGP can find `llvm-strip`. Without it the packaging step gives up
    // on stripping and says so, and the APK carries the whole symbol table of a
    // library nobody is going to debug on a phone.
    ndkVersion = "28.2.13676358"

    defaultConfig {
        applicationId = "org.loopslcr.app"
        minSdk = 26
        targetSdk = 36
        // How many commits deep this build is.
        //
        // It was 1 forever, which meant every build looked to Android like the
        // same version as the one already installed. An installer is entitled
        // to treat that as nothing to do, and "I installed it and nothing
        // changed" is then indistinguishable from a stale APK — which is a
        // question that has already cost this project two rounds.
        versionCode = providers.exec {
            commandLine("git", "rev-list", "--count", "HEAD")
        }.standardOutput.asText.get().trim().toIntOrNull() ?: 1
        // The commit this APK was built from, shown in the app.
        //
        // Not decoration. A release build once reported BUILD SUCCESSFUL and
        // shipped the previous commit's APK — the tell was an output of exactly
        // the same byte count as the build before it, which is easy to miss and
        // was missed. When "is this the new one?" is a question, guessing at it
        // wastes far more time than printing the answer.
        //
        // It only half worked: it caught a stale APK the second time, but it
        // also read `aebd2e8` off a build that predated the commit, because a
        // commit hash alone cannot say whether the sources went in with it. So
        // it carries a "+dirty" when the app's own sources differ from what the
        // named commit holds — the exact state in which the stamp lies.
        versionName = "0.1.0+" + (
            providers.exec {
                commandLine("git", "rev-parse", "--short", "HEAD")
            }.standardOutput.asText.get().trim().ifEmpty { "unknown" }
            ) + (
            providers.exec {
                commandLine(
                    "git", "status", "--porcelain", "--",
                    projectDir.resolve("src").absolutePath,
                )
            }.standardOutput.asText.get().trim().let { if (it.isEmpty()) "" else "+dirty" }
            )
        ndk { abiFilters += abis }
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    /**
     * Release signing, if there is a key to sign with.
     *
     * Read from `keystore.properties`, which is not in the repository and never
     * will be — a signing key in version control is a key anyone who clones can
     * publish updates with. Without the file the release build is simply
     * unsigned, which still builds and still proves the shrinker rules work.
     */
    val keystore = rootProject.file("keystore.properties")
    val credentials = Properties().apply {
        if (keystore.exists()) keystore.inputStream().use { load(it) }
    }

    signingConfigs {
        if (keystore.exists()) {
            create("release") {
                storeFile = file(credentials.getProperty("storeFile"))
                storePassword = credentials.getProperty("storePassword")
                keyAlias = credentials.getProperty("keyAlias")
                keyPassword = credentials.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
            // Only when the suite is being run against this build. See the file.
            if (project.hasProperty("testRelease")) proguardFile("proguard-rules-under-test.pro")
            // The instrumentation APK is shrunk in its own pass and does not
            // inherit the rules above.
            testProguardFiles("proguard-rules-test.pro")
            if (keystore.exists()) signingConfig = signingConfigs.getByName("release")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlin {
        // The JDK the compilers run *on*, provisioned by Gradle. Pinned because
        // the system one is not ours to control: JDK 26 arrived and Kotlin
        // 2.3.21 could not so much as parse its version string.
        jvmToolchain(17)
        compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) }
    }

    // `buildConfig` so the app can show which commit it was built from.
    buildFeatures { compose = true; buildConfig = true }

    /**
     * `./gradlew -PtestRelease connectedAndroidTest` runs the suite against the
     * shrunk build.
     *
     * Worth having as a switch rather than a habit: the release build is the one
     * where R8 has removed everything it could not see used, and the JNI entry
     * points are reachable only through a name the linker resolves at run time.
     * A rule that is wrong fails exactly there and nowhere else.
     */
    testBuildType = if (project.hasProperty("testRelease")) "release" else "debug"

    sourceSets["main"].jniLibs.srcDir(jniLibsDir)

    packaging {
        // The library is already stripped by the NDK linker and page-aligned by
        // AGP; uncompressed lets Android map it instead of unpacking it.
        jniLibs.useLegacyPackaging = false
    }
}

// The .so has to exist before AGP merges native libraries into the APK.
tasks.withType<com.android.build.gradle.tasks.MergeSourceSetFolders>().configureEach {
    if (name.contains("JniLibFolders")) dependsOn(cargoNdk)
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)

    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.graphics)
    implementation(libs.compose.material3)
    implementation(libs.compose.ui.tooling.preview)
    debugImplementation(libs.compose.ui.tooling)

    androidTestImplementation(libs.androidx.test.junit)
    androidTestImplementation(libs.androidx.test.runner)
    androidTestImplementation(platform(libs.compose.bom))
    androidTestImplementation(libs.compose.ui.test.junit4)
    // Declares the empty host activity `createComposeRule` launches into. It is
    // a debug-only artefact by design; under `-PtestRelease` the release build
    // needs it too, or every Compose test fails with "unable to resolve
    // activity" — a missing manifest entry, not a shrinking problem.
    debugImplementation(libs.compose.ui.test.manifest)
    if (project.hasProperty("testRelease")) {
        add("releaseImplementation", libs.compose.ui.test.manifest)
    }
}
