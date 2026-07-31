package org.loopslcr;

import java.nio.ByteBuffer;
import java.nio.file.Files;
import java.nio.file.Path;

/**
 * Exercises the bridge from the side that matters.
 *
 * <p>The Rust tests already cover what every call decides. What they cannot
 * cover is the boundary itself: whether a direct buffer arrives intact, whether
 * a returned array holds the right bytes, and above all whether a Rust panic
 * becomes a Java exception instead of a corrupted runtime. Those only exist once
 * there is a real JVM with the real library loaded.
 *
 * <p>Deliberately no test framework: one class, a main method, and a
 * non-zero exit on failure. Adding JUnit would mean a dependency resolver and a
 * downloaded jar to prove that a shared library loads.
 */
public final class BridgeTest {
    private static int failures = 0;

    public static void main(String[] args) throws Exception {
        System.loadLibrary("loopslcr_jni");
        byte[] wav = Files.readAllBytes(Path.of(args[0]));

        check("the library loaded and the names match", !Native.version().isEmpty());

        // --- analyze ------------------------------------------------------
        String analysis = Native.analyze(Native.direct(wav), "200 loop.wav");
        check("analysis is JSON", analysis.startsWith("{") && analysis.endsWith("}"));
        check("analysis reports the sample rate", analysis.contains("\"sampleRate\":8000"));
        check("analysis reports the tempo from the name", analysis.contains("\"nameTempo\":200.0"));
        check("analysis reports the loop length", analysis.contains("\"loopBars\":8"));

        // --- plan ---------------------------------------------------------
        String plan = Native.plan(Native.direct(wav), "200 loop.wav", "{\"targetBpm\":100}");
        check("plan reports the resulting tempo", plan.contains("\"resultingTempo\":100.0"));
        check("plan reports an exact ratio", plan.contains("\"ratioExact\":true"));

        // --- process ------------------------------------------------------
        byte[] out = Native.process(Native.direct(wav), "200 loop.wav", "{\"depth\":\"16\"}");
        check("process returned a RIFF file", out.length > 44
                && out[0] == 'R' && out[1] == 'I' && out[2] == 'F' && out[3] == 'F');
        check("process returned WAVE", out[8] == 'W' && out[9] == 'A' && out[10] == 'V' && out[11] == 'E');
        check("16-bit output is smaller than a 24-bit source", out.length < wav.length);

        // The same call twice must produce the same bytes: the reproducibility
        // invariant has to survive the boundary too.
        byte[] again = Native.process(Native.direct(wav), "200 loop.wav", "{\"depth\":\"16\"}");
        check("two identical calls give identical bytes", java.util.Arrays.equals(out, again));

        // --- peaks --------------------------------------------------------
        float[] peaks = Native.peaks(Native.direct(wav), 64);
        check("peaks are two channels of min and max", peaks.length == 64 * 2 * 2);
        boolean anySignal = false;
        for (float v : peaks) {
            check("no peak is NaN", !Float.isNaN(v));
            if (v > 0.2f) anySignal = true;
        }
        check("the peaks contain signal", anySignal);

        // --- calculator ---------------------------------------------------
        String sums = Native.calculate("{\"bpm\":120,\"sampleRate\":48000}");
        check("the calculator answers", sums.contains("\"samplesPerBar\":96000.0"));
        check("and returns the note table", sums.contains("\"label\":\"1/8.\""));
        check("a calculator without a tempo throws", throwsIllegalState(() ->
                Native.calculate("{\"bars\":4}")));

        // --- preview ------------------------------------------------------
        long preview = Native.previewCreate(Native.direct(wav), "200 loop.wav", "{}");
        check("a preview handle is not zero", preview != 0);

        String info = Native.previewInfo(preview);
        check("the preview knows its format", info.contains("\"sampleRate\":8000")
                && info.contains("\"channels\":2"));

        // Interleaved floats into a direct buffer: the transfer an AudioTrack
        // does thousands of times a minute, so it is the one worth checking.
        java.nio.ByteBuffer block = java.nio.ByteBuffer
                .allocateDirect(256 * 2 * 4)
                .order(java.nio.ByteOrder.nativeOrder());
        check("a block of 256 frames comes back", Native.previewRead(preview, block, 256) == 256);
        java.nio.FloatBuffer floats = block.asFloatBuffer();
        boolean audible = false;
        for (int i = 0; i < 256 * 2; i++) {
            float v = floats.get(i);
            check("no preview sample is NaN", !Float.isNaN(v));
            if (Math.abs(v) > 0.01f) audible = true;
        }
        check("the preview produced sound", audible);
        check("the play head moved", Native.previewInfo(preview).contains("\"played\":256"));

        Native.previewSetRatio(preview, 1.5);
        Native.previewRead(preview, block, 256);
        Native.previewSeek(preview, 0.0);
        check("seeking put the head back", Native.previewInfo(preview).contains("\"position\":0.0"));

        check("asking a buffer for more than it holds throws", throwsIllegalState(() ->
                Native.previewRead(preview, block, 100_000)));
        check("a zero handle throws rather than crashing", throwsIllegalState(() ->
                Native.previewInfo(0)));

        Native.previewDestroy(preview);
        // Tearing down twice is what a UI does when it stops and then closes.
        Native.previewDestroy(0);
        check("the runtime survived the teardown", !Native.version().isEmpty());

        // --- errors -------------------------------------------------------
        check("a bad file throws", throwsIllegalState(() ->
                Native.analyze(Native.direct("not a wave file".getBytes()), "x.wav")));
        check("an unknown parameter throws", throwsIllegalState(() ->
                Native.process(Native.direct(wav), "200 loop.wav", "{\"targetBPM\":90}")));
        check("a non-direct buffer throws rather than crashing", throwsIllegalState(() ->
                Native.analyze(ByteBuffer.allocate(16), "x.wav")));
        check("a null buffer throws", throwsIllegalState(() -> Native.analyze(null, "x.wav")));
        check("a null name throws", throwsIllegalState(() ->
                Native.analyze(Native.direct(wav), null)));

        // The one that cannot be tested from Rust: unwinding into the JVM is
        // undefined behaviour, so the guard has to be proved from this side.
        check("a Rust panic becomes an exception", throwsIllegalState(Native::panicOnPurpose));

        // …and the JVM is still usable afterwards, which is the actual claim.
        check("the runtime survived the panic", !Native.version().isEmpty());
        String after = Native.analyze(Native.direct(wav), "200 loop.wav");
        check("and still works", after.equals(analysis));

        if (failures > 0) {
            System.out.println(failures + " check(s) failed");
            System.exit(1);
        }
        System.out.println("all bridge checks passed");
    }

    private static boolean throwsIllegalState(Runnable body) {
        try {
            body.run();
            return false;
        } catch (IllegalStateException e) {
            // A message is part of the contract: a bare exception tells the user
            // nothing about which file or which parameter was wrong.
            return e.getMessage() != null && !e.getMessage().isEmpty();
        }
    }

    private static void check(String what, boolean ok) {
        if (!ok) {
            System.out.println("FAILED: " + what);
            failures++;
        }
    }
}
