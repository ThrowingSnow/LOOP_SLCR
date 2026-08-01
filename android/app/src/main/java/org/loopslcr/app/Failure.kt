package org.loopslcr.app

/**
 * What to tell the user when something went wrong.
 *
 * # Why `Throwable` and not `Exception`
 *
 * `OutOfMemoryError` is an `Error`, not an `Exception`, so a `catch (e:
 * Exception)` lets it through and the app dies. That is not a hypothetical
 * here: the whole file is held in memory and decoded to 64-bit samples, so a
 * five-minute stereo file at 48 kHz becomes about 230 MB before the pipeline
 * has made a single copy of it. On a phone that is the difference between a
 * message and a crash.
 *
 * Catching an `OutOfMemoryError` is normally a bad idea, because the state
 * afterwards is unknowable. It is defensible here for one reason: everything
 * the allocation belonged to is dropped on the same path. The file, the
 * decoded buffer and the plan all go, and what is left is a UI with no file
 * open — which is exactly the state the app starts in.
 */
fun explain(e: Throwable, fallback: String): String = when (e) {
    is OutOfMemoryError ->
        "not enough memory for this file — it is decoded whole, which takes " +
            "roughly twenty times its size on disk. A shorter file will work."
    else -> e.message?.takeIf { it.isNotBlank() } ?: fallback
}
