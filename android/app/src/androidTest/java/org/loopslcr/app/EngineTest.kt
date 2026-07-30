package org.loopslcr.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.nio.ByteBuffer
import java.nio.ByteOrder
import kotlin.math.sin

/**
 * The bridge, on Android.
 *
 * The host JVM test already proves the boundary works when the library is built
 * for this machine. What it cannot prove is that the same thing holds once the
 * library has been cross-compiled, packaged into an APK, stripped by the build,
 * and loaded by Android's linker instead of glibc's. That is what runs here.
 *
 * Deliberately no UI automation: driving a file picker from a test says more
 * about the test framework than about this app. What is worth checking is that
 * a real file goes in and a real loop comes out, on a real Android runtime.
 */
@RunWith(AndroidJUnit4::class)
class EngineTest {

    private val name = "200 loop.wav"
    private val wav = wav(bars = 8)

    @Test
    fun the_library_loads_and_reports_its_version() {
        assertTrue(Engine.version.isNotEmpty())
    }

    @Test
    fun a_file_is_analyzed_the_way_its_name_says() {
        val a = Engine.analyze(wav, name)
        assertEquals(2, a.channels)
        assertEquals(RATE, a.sampleRate)
        assertEquals(8L * BAR, a.frames)
        // 200 comes from the file name; nothing in the file declares it.
        assertEquals(200.0, a.nameTempo!!, 0.0)
        assertEquals(8L, a.loopBars)
    }

    @Test
    fun the_waveform_buckets_have_the_shape_the_drawing_expects() {
        val peaks = Engine.peaks(wav, 64)
        assertEquals(64 * 2 * 2, peaks.size)
        assertTrue(peaks.none { it.isNaN() })
        assertTrue(peaks.any { it > 0.2f })
    }

    @Test
    fun halving_the_tempo_is_exact() {
        val plan = Engine.plan(wav, name, Settings(speedMode = SpeedMode.TargetBpm, targetBpm = 100.0))
        assertEquals(100.0, plan.resultingTempo, 0.0)
        assertTrue(plan.ratioExact)
        // Half the speed over the same eight bars is twice the frames, exactly.
        assertEquals(16L * BAR, plan.outputFrames)
    }

    @Test
    fun a_cut_comes_back_as_a_wave_file() {
        val out = Engine.process(wav, name, Settings(depth = "16"))
        assertTrue(out.size > 44)
        assertEquals("RIFF", String(out.copyOfRange(0, 4)))
        assertEquals("WAVE", String(out.copyOfRange(8, 12)))
    }

    @Test
    fun the_same_settings_give_the_same_bytes() {
        // Invariant 4 has to survive the boundary, the cross-compiler and the
        // device: the dither is seeded, so twice is bit-identical or the
        // reproducibility claim is only true on the machine that made it.
        val once = Engine.process(wav, name, Settings(depth = "16"))
        val twice = Engine.process(wav, name, Settings(depth = "16"))
        assertTrue(once.contentEquals(twice))
    }

    @Test
    fun tape_character_changes_the_sound_and_not_the_length() {
        val plain = Engine.process(wav, name, Settings())
        val taped = Engine.process(wav, name, Settings(tape = true))
        assertFalse(plain.contentEquals(taped))
        // The wobble displaces positions rather than changing the rate, so the
        // loop is still exactly as long as it was. That is the whole reason it
        // is safe to put on a loop at all.
        assertEquals(plain.size, taped.size)
    }

    @Test
    fun a_bad_file_fails_with_something_readable() {
        val e = runCatching { Engine.analyze("not a wave file".toByteArray(), "x.wav") }.exceptionOrNull()
        assertTrue(e is IllegalStateException)
        assertTrue(!e!!.message.isNullOrEmpty())
    }

    @Test
    fun a_panic_becomes_an_exception_here_too() {
        // The guard was proved on the host. Android uses a different runtime and
        // a different unwinder, so the claim is worth making again where it will
        // actually be relied on.
        val e = runCatching { org.loopslcr.Native.panicOnPurpose() }.exceptionOrNull()
        assertTrue(e is IllegalStateException)
        assertTrue(Engine.version.isNotEmpty())
    }

    companion object {
        const val RATE = 8_000

        /** One bar of 4/4 at 200 BPM at [RATE]. Exact, deliberately. */
        const val BAR = 9_600L

        /**
         * A test loop, encoded by hand.
         *
         * Sixteen-bit stereo PCM and a 44-byte header — the smallest thing that
         * is unambiguously a WAVE file. Written here rather than shipped as an
         * asset so the fixture is readable: a binary in the tree would hide what
         * the test is actually feeding in.
         */
        fun wav(bars: Int): ByteArray {
            val frames = (bars * BAR).toInt()
            val dataBytes = frames * 2 * 2
            val out = ByteBuffer.allocate(44 + dataBytes).order(ByteOrder.LITTLE_ENDIAN)
            out.put("RIFF".toByteArray()).putInt(36 + dataBytes).put("WAVE".toByteArray())
            out.put("fmt ".toByteArray()).putInt(16)
            out.putShort(1).putShort(2)
            out.putInt(RATE).putInt(RATE * 4).putShort(4).putShort(16)
            out.put("data".toByteArray()).putInt(dataBytes)
            for (i in 0 until frames) {
                val v = (0.25 * sin(i * 0.05) * Short.MAX_VALUE).toInt().toShort()
                out.putShort(v).putShort(v)
            }
            return out.array()
        }
    }
}
