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
        versionCode = 1
        versionName = "0.1.0"
        ndk { abiFilters += abis }
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    buildTypes {
        release {
            // No shrinking yet: R8 would strip the JNI entry points unless told
            // otherwise, and a rule kept honest needs an APK to test it against.
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlin {
        compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) }
    }

    buildFeatures { compose = true }

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
    debugImplementation(libs.compose.ui.test.manifest)
}
