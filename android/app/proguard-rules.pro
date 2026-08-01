# The JNI boundary, kept by name.
#
# R8 renames and removes what it cannot see used. Nothing in the Kotlin calls
# `Java_org_loopslcr_Native_analyze` — the *linker* does, at run time, by
# matching a symbol in the shared library against a class and method name. R8
# cannot see that, so without these rules a release build shrinks perfectly,
# installs perfectly, and throws UnsatisfiedLinkError on the first file opened.
#
# Two rules rather than one, because they answer different questions:
#   - keep the class, so `org/loopslcr/Native` still exists under that name;
#   - keep the members, so the method names the symbols encode still match.
-keep class org.loopslcr.Native { *; }
-keepclasseswithmembernames class * {
    native <methods>;
}

# Line numbers survive, so a crash report from a release build names a line.
# Costs a few kilobytes and is the difference between a bug report and a shrug.
-keepattributes SourceFile,LineNumberTable
-renamesourcefileattribute SourceFile

# Error Prone's annotations are compile-time only and are on no runtime
# classpath. R8 meets the reference through androidx.test when the suite is run
# against the shrunk build (`-PtestRelease`), and refuses to guess. Saying so
# here is narrower than turning missing-class errors off wholesale.
-dontwarn com.google.errorprone.annotations.**
