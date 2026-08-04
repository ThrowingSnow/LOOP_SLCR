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
    private val raw = wav(bars = 8)

    /** A fresh direct buffer each time: the tests hand the same file to many calls. */
    private val wav: ByteBuffer get() = org.loopslcr.Native.direct(raw)

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
        val e = runCatching { Engine.analyze(org.loopslcr.Native.direct("not a wave file".toByteArray()), "x.wav") }.exceptionOrNull()
        assertTrue(e is IllegalStateException)
        assertTrue(!e!!.message.isNullOrEmpty())
    }

    @Test
    fun a_preview_produces_sound_and_frees_cleanly() {
        val handle = org.loopslcr.Native.previewCreate(wav, name, "{}")
        assertTrue(handle != 0L)
        try {
            val info = org.json.JSONObject(org.loopslcr.Native.previewInfo(handle))
            assertEquals(2, info.getInt("channels"))
            assertEquals(RATE, info.getInt("sampleRate"))

            val block = ByteBuffer.allocateDirect(256 * 2 * 4).order(ByteOrder.nativeOrder())
            assertEquals(256, org.loopslcr.Native.previewRead(handle, block, 256))
            val floats = block.asFloatBuffer()
            var audible = false
            for (i in 0 until 256 * 2) {
                val v = floats.get(i)
                assertFalse(v.isNaN())
                if (kotlin.math.abs(v) > 0.01f) audible = true
            }
            assertTrue("the preview produced silence", audible)
        } finally {
            org.loopslcr.Native.previewDestroy(handle)
        }
    }

    @Test
    fun the_preview_plays_the_whole_loop_and_comes_back_round() {
        // Reading exactly twice the loop length must land back where it started.
        // A preview that drifts by a sample per pass is a preview that lies
        // about the one property the tool exists to guarantee.
        val handle = org.loopslcr.Native.previewCreate(wav, name, "{}")
        try {
            val frames = org.json.JSONObject(org.loopslcr.Native.previewInfo(handle)).getLong("frames")
            val block = ByteBuffer.allocateDirect(1024 * 2 * 4).order(ByteOrder.nativeOrder())
            var done = 0L
            while (done < frames * 2) {
                val want = minOf(1024L, frames * 2 - done).toInt()
                done += org.loopslcr.Native.previewRead(handle, block, want)
            }
            val position = org.json.JSONObject(org.loopslcr.Native.previewInfo(handle))
                .getDouble("position")
            assertTrue("came back to $position", kotlin.math.abs(position) < 1e-6)
        } finally {
            org.loopslcr.Native.previewDestroy(handle)
        }
    }

    @Test
    fun the_motion_rearranges_the_loop_and_still_repeats_it() {
        // End to end, through the bridge and on the device: the claim the whole
        // feature rests on. The play head is displaced by whole pieces of the
        // loop, so what comes out is rearranged — and because the pattern is a
        // function of which piece you are in rather than of elapsed time, the
        // next time round is the same audio again. Rearranged *and* still a
        // loop, or it is not this feature.
        val handle = org.loopslcr.Native.previewCreate(wav, name, "{}")
        try {
            val frames = org.json.JSONObject(org.loopslcr.Native.previewInfo(handle))
                .getLong("frames").toInt()
            // Eight pieces, reaching up to three of them, scattered.
            org.loopslcr.Native.previewSetMotion(handle, true, 8, 3, 1, 3)

            // Three passes: the first starts cold with nothing to cross-fade
            // from, so the period is checked between the second and the third.
            val passes = (0 until 3).map { readWholeLoop(handle, frames) }

            val plain = org.loopslcr.Native.previewCreate(wav, name, "{}")
            val straight = try {
                readWholeLoop(plain, frames)
            } finally {
                org.loopslcr.Native.previewDestroy(plain)
            }

            var moved = 0
            for (i in straight.indices) {
                if (kotlin.math.abs(passes[0][i] - straight[i]) > 1e-4f) moved++
            }
            assertTrue("the motion changed nothing at all", moved > frames / 4)

            for (i in passes[1].indices) {
                assertEquals(
                    "the loop stopped repeating at sample $i",
                    passes[1][i],
                    passes[2][i],
                    1e-5f,
                )
            }

            // And switching it off puts the audio back where the clock is.
            org.loopslcr.Native.previewSetMotion(handle, false, 8, 3, 1, 3)
            readWholeLoop(handle, frames)
            val info = org.json.JSONObject(org.loopslcr.Native.previewInfo(handle))
            assertEquals(
                "still displaced after being switched off",
                info.getDouble("position"),
                info.getDouble("sounding"),
                1e-6,
            )
        } finally {
            org.loopslcr.Native.previewDestroy(handle)
        }
    }

    @Test
    fun a_second_loop_is_cut_to_the_first_one_s_length_and_not_to_its_tempo() {
        // Reported from the phone: "CUTTER 2 does not run with the swap", with
        // `the second loop is a different length — 769745 frames against
        // 769739`. Six frames, on a pair whose tempi were both known exactly.
        //
        // The tempo route rounds twice on the way to a length — once to a bar
        // grid, once to a frame — and the first loop's own length had been
        // reached by a different path. So the request is now the length itself.
        val handle = org.loopslcr.Native.previewCreate(wav, name, "{}")
        try {
            val frames = org.json.JSONObject(org.loopslcr.Native.previewInfo(handle))
                .getLong("frames")

            // A tempo that lands nowhere near the first loop's length, so the
            // only thing that can make this fit is the length itself.
            org.loopslcr.Native.previewSetPartner(
                handle,
                org.loopslcr.Native.direct(wav(bars = 8, tone = 0.11)),
                "200 other.wav",
                Settings(
                    bars = 8L,
                    speedMode = SpeedMode.TargetBpm,
                    targetBpm = 190.0,
                    targetFrames = frames,
                ).toJson(),
            )
            assertTrue(
                "the partner did not take",
                org.json.JSONObject(org.loopslcr.Native.previewInfo(handle))
                    .getBoolean("hasPartner"),
            )
        } finally {
            org.loopslcr.Native.previewDestroy(handle)
        }
    }

    @Test
    fun a_second_loop_alternates_with_the_first_and_shares_its_clock() {
        // The pair, end to end on the device. Two files of the same length at
        // different tempi in their names, so the second is pulled to the first
        // by the same exact arithmetic as any other cut — which is what makes
        // the shared play head possible at all.
        val handle = org.loopslcr.Native.previewCreate(wav, name, "{}")
        try {
            val frames = org.json.JSONObject(org.loopslcr.Native.previewInfo(handle))
                .getLong("frames").toInt()

            val alone = readWholeLoop(handle, frames)

            // The same eight bars, pulled to the same tempo: the same length,
            // exactly, or the bridge refuses it.
            org.loopslcr.Native.previewSetPartner(
                handle,
                org.loopslcr.Native.direct(wav(bars = 8, tone = 0.11)),
                "200 other.wav",
                Settings(bars = 8L, speedMode = SpeedMode.TargetBpm, targetBpm = 200.0).toJson(),
            )
            assertTrue(
                "the partner did not take",
                org.json.JSONObject(org.loopslcr.Native.previewInfo(handle))
                    .getBoolean("hasPartner"),
            )

            // Two pieces on each side of a four-piece grid: the second half of
            // the loop comes from the other file.
            org.loopslcr.Native.previewSetPair(handle, true, 4, 2, 2)
            val paired = readWholeLoop(handle, frames)

            var different = 0
            for (i in alone.indices) {
                if (kotlin.math.abs(alone[i] - paired[i]) > 1e-4f) different++
            }
            assertTrue("the second loop was never heard", different > 0)

            // And the clock is still one clock: a whole pass leaves the head
            // exactly where it started, swaps or no swaps.
            val position = org.json.JSONObject(org.loopslcr.Native.previewInfo(handle))
                .getDouble("position")
            assertTrue("a pass ended at $position", kotlin.math.abs(position) < 1e-6)

            org.loopslcr.Native.previewClearPartner(handle)
            readWholeLoop(handle, frames)
            assertFalse(
                "the partner survived being cleared",
                org.json.JSONObject(org.loopslcr.Native.previewInfo(handle))
                    .getBoolean("hasPartner"),
            )
        } finally {
            org.loopslcr.Native.previewDestroy(handle)
        }
    }

    @Test
    fun a_second_loop_of_the_wrong_length_is_refused_with_both_numbers() {
        // Refused rather than stretched: stretching here would undo the
        // exactness the whole tool is built on. And the message has to carry
        // the numbers, or the user is told something is wrong with no way to
        // find out what.
        val handle = org.loopslcr.Native.previewCreate(wav, name, "{}")
        try {
            val e = runCatching {
                org.loopslcr.Native.previewSetPartner(
                    handle,
                    org.loopslcr.Native.direct(wav(bars = 8)),
                    "200 other.wav",
                    Settings(bars = 4L).toJson(),
                )
            }.exceptionOrNull()

            assertTrue("a half-length partner was accepted", e is IllegalStateException)
            val message = e!!.message ?: ""
            assertTrue("no length in \"$message\"", message.contains("length"))
            assertTrue("no frame counts in \"$message\"", message.contains("frames against"))
        } finally {
            org.loopslcr.Native.previewDestroy(handle)
        }
    }

    /** One loop's worth of interleaved output, in one array. */
    private fun readWholeLoop(handle: Long, frames: Int): FloatArray {
        val chunk = 1024
        val block = ByteBuffer.allocateDirect(chunk * 2 * 4).order(ByteOrder.nativeOrder())
        val out = FloatArray(frames * 2)
        var done = 0
        while (done < frames) {
            val want = minOf(chunk, frames - done)
            val got = org.loopslcr.Native.previewRead(handle, block, want)
            block.rewind()
            val floats = block.asFloatBuffer()
            for (i in 0 until got * 2) out[done * 2 + i] = floats.get(i)
            done += got
        }
        return out
    }

    @Test
    fun a_dead_handle_throws_rather_than_corrupting_anything() {
        val e = runCatching { org.loopslcr.Native.previewInfo(0) }.exceptionOrNull()
        assertTrue(e is IllegalStateException)
        // Freeing nothing is what a UI does when it stops twice.
        org.loopslcr.Native.previewDestroy(0)
        assertTrue(Engine.version.isNotEmpty())
    }

    @Test
    fun the_player_starts_stops_and_can_be_stopped_twice() {
        // AudioTrack on an emulator has no real output device worth trusting,
        // so what is checked is the lifecycle: it starts without throwing, and
        // stopping joins the audio thread before freeing the handle. Getting
        // that order wrong is a use-after-free that would only show as a random
        // crash on a device.
        val player = PreviewPlayer()
        val problem = player.start(wav, name, Settings(), 1.0)
        assertEquals(null, problem)
        assertTrue(player.isPlaying)
        Thread.sleep(200)
        player.setRatio(1.5)
        Thread.sleep(100)
        player.stop()
        assertFalse(player.isPlaying)
        player.stop()
    }

    /**
     * The bug a tape slider found: the loop playing over itself.
     *
     * Dragging wow or flutter invalidates the preview on every notch, and a
     * rebuild runs the whole pipeline — longer than the replan debounce, so a
     * second rebuild starts while the first is still going. `start` began by
     * stopping whatever played, and two callers interleaving in that read-then-
     * replace both found nothing to stop: two `AudioTrack`s, two pump threads,
     * one set of fields remembering only the later. The earlier one could then
     * never be stopped, and played on, out of phase, over the top.
     *
     * Counting threads rather than listening, because an emulator's audio output
     * is not worth an assertion but the thread that feeds it is exactly the
     * thing that must be unique.
     */
    @Test
    fun starting_from_several_threads_at_once_leaves_one_audio_thread() {
        val player = PreviewPlayer()
        try {
            val racers = List(4) {
                Thread { player.start(wav, name, Settings(tape = true), 1.0) }
            }
            racers.forEach { it.start() }
            racers.forEach { it.join(30_000) }

            assertTrue(player.isPlaying)
            assertEquals("one loop playing, not several", 1, pumpThreads())
        } finally {
            player.stop()
        }
        assertEquals("stopping left an audio thread behind", 0, pumpThreads())
    }

    /** Live pump threads, by the name [PreviewPlayer] gives them. */
    private fun pumpThreads(): Int =
        Thread.getAllStackTraces().keys.count { it.name == "loopslcr-preview" && it.isAlive }

    @Test
    fun the_calculator_and_the_cutter_agree_about_the_same_file() {
        // The reason the calculator is a native call. Both screens are asked
        // about the same 200 BPM loop, and the bar length one reports has to be
        // the one the other actually cut with — not a number that resembles it.
        val plan = Engine.plan(wav, name, Settings())
        val sums = Calculator.compute(
            CalculatorSettings(bpm = "200", sampleRate = RATE, bars = plan.bars),
        ).getOrThrow()

        assertEquals(BAR.toDouble(), sums.samplesPerBar, 0.0)
        assertTrue(sums.barSampleExact)
        assertEquals(plan.loopFrames, sums.totalSamplesRounded)
    }

    @Test
    fun the_note_table_arrives_whole() {
        val sums = Calculator.compute(CalculatorSettings(bpm = "120", sampleRate = 48_000))
            .getOrThrow()
        assertEquals(18, sums.notes.size)
        val quarter = sums.notes.first { it.label == "1/4" }
        assertEquals(500.0, quarter.ms, 1e-9)
        assertEquals(24_000.0, quarter.samples, 0.0)
        assertTrue(quarter.sampleExact)
    }

    @Test
    fun a_calculator_without_a_tempo_fails_rather_than_guessing() {
        assertTrue(Calculator.compute(CalculatorSettings(bpm = "")).isFailure)
    }

    @Test
    fun a_file_with_no_tempo_anywhere_is_refused_until_one_is_given() {
        // Fourteen of the 279 archive files are like this. Before the cutter had
        // a BPM field they could not be cut on the phone at all: the pipeline
        // refuses, correctly, and there was nothing to answer it with.
        val nameless = "drums.wav"
        val e = runCatching { Engine.plan(wav, nameless, Settings()) }.exceptionOrNull()
        assertTrue(e is IllegalStateException)
        assertTrue("$e", e!!.message!!.contains("tempo"))

        val plan = Engine.plan(wav, nameless, Settings(bpm = "200"))
        assertEquals(200.0, plan.tempo, 0.0)
        assertEquals("given", plan.tempoSource)
        assertEquals(8L * BAR, plan.loopFrames)
    }

    @Test
    fun a_typed_tempo_beats_the_one_in_the_name() {
        val plan = Engine.plan(wav, name, Settings(bpm = "100"))
        assertEquals(100.0, plan.tempo, 0.0)
        assertEquals("given", plan.tempoSource)
        // Half the tempo is twice the bar, so the same audio reads as four bars
        // instead of eight — same total frames, a different grid under them.
        // That the *count* changed is what proves the override reached the grid.
        assertEquals(4L, plan.bars)
        assertEquals(8L * BAR, plan.loopFrames)
    }

    @Test
    fun a_blank_tempo_leaves_the_file_to_speak_for_itself() {
        val plan = Engine.plan(wav, name, Settings(bpm = "   "))
        assertEquals(200.0, plan.tempo, 0.0)
        assertEquals("filename", plan.tempoSource)
    }

    @Test
    fun grid_alignment_is_reachable_and_changes_the_region() {
        val loop = Engine.plan(wav, name, Settings(bars = 3L, skip = 1L))
        val grid = Engine.plan(wav, name, Settings(bars = 3L, skip = 1L, align = "grid"))
        assertEquals(loop.regionStart, grid.regionStart)
        // At 200 BPM and 8 kHz a bar is exactly 9600 frames, so the two agree
        // here — what is checked is that the parameter is accepted and applied,
        // not that it always differs.
        assertEquals(3L * BAR, loop.loopFrames)
        assertEquals(3L * BAR, grid.loopFrames)
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
        fun wav(bars: Int, tone: Double = 0.05): ByteArray {
            val frames = (bars * BAR).toInt()
            val dataBytes = frames * 2 * 2
            val out = ByteBuffer.allocate(44 + dataBytes).order(ByteOrder.LITTLE_ENDIAN)
            out.put("RIFF".toByteArray()).putInt(36 + dataBytes).put("WAVE".toByteArray())
            out.put("fmt ".toByteArray()).putInt(16)
            out.putShort(1).putShort(2)
            out.putInt(RATE).putInt(RATE * 4).putShort(4).putShort(16)
            out.put("data".toByteArray()).putInt(dataBytes)
            for (i in 0 until frames) {
                val v = (0.25 * sin(i * tone) * Short.MAX_VALUE).toInt().toShort()
                out.putShort(v).putShort(v)
            }
            return out.array()
        }
    }
}
