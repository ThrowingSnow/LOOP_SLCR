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
    /**
     * Loads the library when this class is first touched.
     *
     * <p>It belongs here, on the class that declares the methods, rather than on
     * whichever wrapper happened to be written first: any caller reaching a
     * native method has by definition initialised this class, so there is no
     * path that can forget. It used to live in a Kotlin object, and a second
     * caller that did not go through that object found the gap immediately —
     * as an {@code UnsatisfiedLinkError} at the first call, not at startup.
     *
     * <p>Deliberately not wrapped in a {@code try}: without the library the app
     * is not degraded, it is absent, and failing at class load says so more
     * clearly than a screen full of controls that cannot work.
     */
    static {
        System.loadLibrary("loopslcr_jni");
    }

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

    /**
     * The calculator: beat and bar lengths and the note-value table, as JSON.
     *
     * <p>Native rather than Kotlin arithmetic on purpose. Both tabs then read
     * their numbers off the same grid, and the calculator cannot drift away from
     * the cutter it sits beside.
     */
    public static native String calculate(String paramsJson);

    /**
     * Opens a preview of the loop these parameters describe.
     *
     * <p>Returns an opaque handle, never zero on success. The varispeed in the
     * parameters is deliberately <em>not</em> baked in — the rate is set on the
     * handle instead, so riding it does not rebuild the loop.
     */
    public static native long previewCreate(ByteBuffer audio, String name, String paramsJson);

    /**
     * Fills a direct buffer with interleaved native-endian floats.
     *
     * <p>This is the call the audio thread makes. It neither allocates nor waits
     * on anything the UI thread holds.
     */
    public static native int previewRead(long handle, ByteBuffer out, int frames);

    /** Asks for a new speed. Lock-free; callable from any thread, at any time. */
    public static native void previewSetRatio(long handle, double ratio);

    /**
     * Sets the stepped displacement of the play head, or turns it off.
     *
     * <p>The loop is divided into {@code steps} equal pieces and the head is
     * displaced by up to {@code depth} whole pieces, so what comes out always
     * starts where a piece starts and the loop still repeats. {@code every} is
     * how many pieces pass between moves — the grid says where a jump may land,
     * this says how often one happens, and counting it in pieces is what makes
     * the rate beat-synced with no number that could fall between two beats.
     * {@code shape} is 0 rise, 1 fall, 2 swing, 3 scatter, 4 walk.
     *
     * <p>Lock-free; callable from any thread. Takes effect at the next boundary,
     * so turning a control while the loop plays does not click.
     */
    public static native void previewSetMotion(
            long handle, boolean on, int steps, int depth, int every, int shape);

    /**
     * Sets the swap schedule between the loop and its partner, or turns it off.
     *
     * <p>The loop is divided into {@code steps} equal pieces; {@code holdA}
     * pieces are read from the first loop and {@code holdB} from the second,
     * over and over. One play head serves both, so the second loop is heard at
     * the same place in the bar the first would have been — nothing is
     * restarted and there are no two clocks to drift apart.
     *
     * <p>Lock-free; callable from any thread. Lands on the next piece boundary.
     */
    public static native void previewSetPair(
            long handle, boolean on, int steps, int holdA, int holdB);

    /**
     * Gives the running preview a second loop.
     *
     * <p>Runs the pipeline, so it takes as long as a cut and belongs on a
     * background thread. Throws if the result is not exactly the same length,
     * rate and channel count as the loop it joins — the two share one play
     * head, and a partner of a different length has no shared phase to be read
     * at. The caller knows both tempi and can ask for one that fits.
     */
    public static native void previewSetPartner(
            long handle, ByteBuffer audio, String name, String paramsJson);

    /**
     * Sets the level trim for each loop, linear.
     *
     * <p>Both in one call: set one at a time and the audio thread could read a
     * block with the new first gain and the old second one, which is a balance
     * nobody asked for right where a swap makes it audible.
     */
    public static native void previewSetGains(long handle, float first, float second);

    /**
     * Sets the trim on the sum, after both loop gains.
     *
     * <p>Its own call because it is its own decision: the loop gains balance
     * the two against each other, this one decides how loud that balance
     * leaves.
     */
    public static native void previewSetMasterGain(long handle, float gain);

    /** Takes the second loop away. The first keeps playing. */
    public static native void previewClearPartner(long handle);

    /** Moves the play head, in source frames. Wraps. */
    public static native void previewSeek(long handle, double frame);

    /** Format, length and play head, as JSON. */
    public static native String previewInfo(long handle);

    /** Frees the handle. It must not be used again; destroying zero is a no-op. */
    public static native void previewDestroy(long handle);

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
