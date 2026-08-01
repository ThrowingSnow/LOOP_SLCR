# Applied to the app only under `-PtestRelease`, never to a shipped build.
#
# # The problem
#
# The instrumentation APK runs inside the app's process and resolves classes
# through the app's classloader. R8 has already removed and renamed everything
# the *app* does not use — so `androidx.test` and `compose-ui-test` reach for
# `androidx.tracing.Trace`, `kotlin.LazyKt`, `MonotonicFrameClock$DefaultImpls`,
# `androidx.collection.mutableIntObjectMapOf`, and each one is gone or renamed.
# Every miss is a `NoClassDefFoundError` or `NoSuchMethodError` before the test
# body runs. R8 also inlines small Kotlin objects into their only caller and
# deletes the class, which takes `Engine`, `Calculator` and `Markers` with it.
#
# Chasing them one at a time is a losing game that produces a rule set saying
# nothing about this app. So under test the libraries and the app's own code are
# kept whole.
#
# # What this run proves, and what it does not
#
# **Proves:** the JNI keep rules in `proguard-rules.pro` are right — those are
# the shipped rules, unmodified, and they are what make `org.loopslcr.Native`
# survive; the `.so` is packaged for the right ABI and loads under Android's
# linker; resource shrinking removed nothing the app needs; and the signed APK
# installs and runs the whole engine, preview included.
#
# **Does not prove:** anything about shrunk or renamed Kotlin, because under
# these keeps almost nothing is shrunk. That gap is acceptable for one reason:
# the only thing in this app whose correctness depends on a *name* surviving is
# the JNI boundary, and that is covered by the rules being tested rather than by
# the ones here.
#
# Writing this down rather than leaving it implied is the point. A green run
# that quietly tested a different APK than the one shipped would be worse than
# no run at all.

-keep class org.loopslcr.app.** { *; }
-keep class androidx.** { *; }
-keep class kotlin.** { *; }
-keep class kotlinx.** { *; }
-dontwarn androidx.**
