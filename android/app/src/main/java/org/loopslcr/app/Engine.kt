package org.loopslcr.app

import org.json.JSONObject
import org.loopslcr.Native
import java.nio.ByteBuffer

/**
 * The native side, wrapped once.
 *
 * Everything the app knows about cutting loops lives in Rust. This object exists
 * so that exactly one place loads the library, exactly one place turns a
 * `byte[]` into the direct buffer the boundary wants, and exactly one place
 * decides what a failure looks like to the UI.
 */
object Engine {
    // The library is loaded by `Native`'s own static initialiser, not here.
    // Loading it in this object meant a caller that reached `Native` without
    // going through `Engine` — the calculator does — found no implementation.

    val version: String get() = Native.version()

    fun analyze(bytes: ByteArray, name: String): Analysis =
        Analysis.from(JSONObject(Native.analyze(direct(bytes), name)))

    fun plan(bytes: ByteArray, name: String, settings: Settings): Plan =
        Plan.from(JSONObject(Native.plan(direct(bytes), name, settings.toJson())))

    fun process(bytes: ByteArray, name: String, settings: Settings): ByteArray =
        Native.process(direct(bytes), name, settings.toJson())

    fun peaks(bytes: ByteArray, buckets: Int): FloatArray = Native.peaks(direct(bytes), buckets)

    /**
     * A direct buffer holding [bytes].
     *
     * The copy is here rather than at the boundary because the boundary must not
     * copy: Rust reads a direct buffer in place. The JVM cannot hand out a
     * pointer into a normal `byte[]` — the collector may move it — so the copy
     * is the price of reading the file without a second one on the native side.
     */
    private fun direct(bytes: ByteArray): ByteBuffer = Native.direct(bytes)
}
