package org.loopslcr.app

import org.json.JSONObject
import org.loopslcr.Native
import java.nio.ByteBuffer

/**
 * The native side, wrapped once.
 *
 * Everything the app knows about cutting loops lives in Rust. This object exists
 * so that exactly one place turns a result into a type the UI can hold, and
 * exactly one place decides what a failure looks like.
 *
 * # Why every call takes a `ByteBuffer`
 *
 * A direct buffer is memory the JVM allocated outside the Java heap, which Rust
 * reads in place. Taking a `ByteArray` here would mean copying the whole file
 * into one on every call — and on a phone the file is the largest thing in the
 * app by an order of magnitude. So the file is read into a direct buffer once,
 * when it is opened, and that buffer is what everything else is handed.
 */
object Engine {
    // The library is loaded by `Native`'s own static initialiser, not here.
    // Loading it in this object meant a caller that reached `Native` without
    // going through `Engine` — the calculator does — found no implementation.

    val version: String get() = Native.version()

    fun analyze(audio: ByteBuffer, name: String): Analysis =
        Analysis.from(JSONObject(Native.analyze(audio, name)))

    fun plan(audio: ByteBuffer, name: String, settings: Settings): Plan =
        Plan.from(JSONObject(Native.plan(audio, name, settings.toJson())))

    fun process(audio: ByteBuffer, name: String, settings: Settings): ByteArray =
        Native.process(audio, name, settings.toJson())

    fun peaks(audio: ByteBuffer, buckets: Int): FloatArray = Native.peaks(audio, buckets)
}
