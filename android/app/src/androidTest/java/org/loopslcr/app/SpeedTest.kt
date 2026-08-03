package org.loopslcr.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The approximate ratio the preview glides towards while a finger is moving.
 *
 * Its whole reason for existing is speed, so the thing worth testing is that
 * being fast has not made it wrong: it must agree with the exact answer closely
 * enough that the correction on arrival is inaudible, and it must refuse rather
 * than guess when it cannot know.
 */
class SpeedTest {

    @Test
    fun semitones_give_the_equal_tempered_ratio() {
        assertEquals(1.0, Speed.ratio(Settings(semitones = 0.0), 103.0)!!, 1e-12)
        assertEquals(2.0, Speed.ratio(Settings(semitones = 12.0), 103.0)!!, 1e-12)
        assertEquals(0.5, Speed.ratio(Settings(semitones = -12.0), 103.0)!!, 1e-12)
        // A fifth up is 1.4983, not 1.5 — equal temperament, same as the core.
        assertEquals(1.498307, Speed.ratio(Settings(semitones = 7.0), 103.0)!!, 1e-6)
    }

    @Test
    fun a_target_tempo_is_the_plain_quotient() {
        val s = Settings(speedMode = SpeedMode.TargetBpm, targetBpm = 90.0)
        assertEquals(90.0 / 103.0, Speed.ratio(s, 103.0)!!, 1e-12)
    }

    @Test
    fun it_agrees_with_the_exact_answer_far_inside_a_cent() {
        // The claim that makes this safe: when the plan arrives with the exact
        // rational, the value it replaces was so close that the 120 ms glide
        // cannot render the correction audible. A cent is 0.0578 %.
        val exact = 90.0 / 103.0
        val mine = Speed.ratio(
            Settings(speedMode = SpeedMode.TargetBpm, targetBpm = 90.0),
            103.0,
        )!!
        assertTrue("off by ${(mine / exact - 1.0) * 100} %", kotlin.math.abs(mine / exact - 1.0) < 1e-9)
    }

    @Test
    fun a_target_tempo_without_a_source_tempo_refuses() {
        // The important null. Falling back to 1.0 would silently claim the pitch
        // is unchanged, which is a lie the user would hear and not understand.
        val s = Settings(speedMode = SpeedMode.TargetBpm, targetBpm = 90.0)
        assertNull(Speed.ratio(s, null))
        assertNull(Speed.ratio(s, 0.0))
        assertNull(Speed.ratio(Settings(speedMode = SpeedMode.TargetBpm, targetBpm = null), 103.0))
    }

    @Test
    fun nothing_unplayable_ever_reaches_the_audio_thread() {
        // The preview clamps anyway; this makes sure the clamp is not the first
        // place a nonsense value is noticed.
        assertTrue(Speed.ratio(Settings(semitones = 96.0), 103.0)!! <= 4.0)
        assertTrue(Speed.ratio(Settings(semitones = -96.0), 103.0)!! >= 0.25)
        assertNull(Speed.ratio(Settings(semitones = Double.NaN), 103.0))
    }
}
