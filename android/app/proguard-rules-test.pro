# Rules for the *test* APK, which R8 shrinks separately from the app.
#
# `proguardFiles` applies to the app; the instrumentation APK gets its own pass
# with `testProguardFiles`, so the app's rules do not reach here. That surprised
# me, which is why it is written down.
#
# Error Prone's annotations are compile-time only and are on no runtime
# classpath. androidx.test references them, and R8 refuses to guess. Naming them
# is narrower than turning missing-class errors off wholesale.
-dontwarn com.google.errorprone.annotations.**

# The tests reach the native methods directly, so the same keep rules apply.
-keep class org.loopslcr.Native { *; }
-keepclasseswithmembernames class * {
    native <methods>;
}

# AndroidJUnitRunner reaches androidx.tracing.Trace reflectively at startup.
# Shrunk away, the whole instrumentation process dies with NoClassDefFoundError
# before a single test runs — which looks like "0 tests" and no explanation.
-keep class androidx.tracing.** { *; }
