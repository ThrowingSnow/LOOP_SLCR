package org.loopslcr;

import java.nio.ByteBuffer;

/**
 * The native call surface, as the Android app will see it.
 *
 * <p>Every method is static and takes a direct {@link ByteBuffer} for PCM: the
 * JVM owns that memory and Rust reads it in place, so a file crosses the
 * boundary without being copied. Results come back as Java arrays, which does
 * copy once — cheap next to decoding and resampling the same file, and it keeps
 * every allocation on the side of the garbage collector that can see it.
 *
 * <p>Failures arrive as {@link IllegalStateException}, including failures that
 * were panics on the Rust side. Nothing unwinds across the boundary.
 */
public final class Native {
    private Native() {}

    public static native String version();

    /** File facts and what can be read off the name, as JSON. */
    public static native String analyze(ByteBuffer audio, String name);

    /** What a cut would do, as JSON, without producing one. */
    public static native String plan(ByteBuffer audio, String name, String paramsJson);

    /** The finished WAVE file. */
    public static native byte[] process(ByteBuffer audio, String name, String paramsJson);

    /** Waveform buckets: [c0min, c0max, c1min, c1max, ...] per bucket. */
    public static native float[] peaks(ByteBuffer audio, int buckets);

    /** Panics on purpose, so the guard can be proved from this side. */
    public static native void panicOnPurpose();

    /** Copies a byte array into the direct buffer the native side requires. */
    public static ByteBuffer direct(byte[] bytes) {
        ByteBuffer buffer = ByteBuffer.allocateDirect(bytes.length);
        buffer.put(bytes);
        buffer.rewind();
        return buffer;
    }
}
