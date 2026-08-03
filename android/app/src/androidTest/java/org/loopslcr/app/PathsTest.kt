package org.loopslcr.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * What the app says about the choice it made.
 *
 * Worth testing precisely because it is only words: a wrong number is caught by
 * arithmetic somewhere, a wrong sentence is caught by nobody. The claim under
 * test is that a decision taken on the user's behalf is always said out loud,
 * and that a disagreement is never silent.
 */
class PathsTest {

    /** A plan is a wide type; only four of its fields matter here. */
    private fun plan(detected: String, chosen: String, audibleBars: Double, bars: Long = 8L) = Plan(
        tempo = 103.0,
        tempoSource = "name",
        bars = bars,
        barsSource = "length",
        workflowDetected = detected,
        workflowChosen = chosen,
        audibleBars = audibleBars,
        barsInFile = audibleBars,
        skipBars = 0L,
        regionStart = 0L,
        regionEnd = 100L,
        loopFrames = 100L,
        shortBy = 0L,
        fadeFrames = 0L,
        ratio = 1.0,
        ratioExact = true,
        semitones = 0.0,
        resultingTempo = 103.0,
        outputFrames = 100L,
        peak = 0.5,
        clips = false,
        peakBeforeVarispeed = false,
        tape = false,
        normalizeGain = null,
        dithered = false,
    )

    @Test
    fun the_buttons_say_what_the_tool_will_do_not_what_the_file_is() {
        // "warmup" describes the render; "cut" describes the action. Someone
        // pressing a button is choosing an action.
        assertEquals("cut (A)", Paths.label("warmup"))
        assertEquals("fold (B)", Paths.label("foldback"))
        assertEquals("auto", Paths.label("auto"))
    }

    @Test
    fun an_unknown_path_never_renders_as_a_raw_name() {
        // A new variant in Rust must not reach the screen as an identifier.
        assertEquals("unclear", Paths.label("something_new"))
        assertTrue(Paths.explain("something_new").isNotEmpty())
    }

    @Test
    fun auto_reports_what_it_detected_with_the_evidence() {
        val note = Paths.note("auto", plan(detected = "foldback", chosen = "foldback", audibleBars = 8.7))
        assertFalse("agreeing with the file is not a warning", note.warn)
        assertTrue(note.text, note.text.contains("fold (B)"))
        // 8.7 audible bars over an 8-bar loop is 1.09 loops — the number the
        // detection thresholds on, shown so it can be disagreed with.
        assertTrue(note.text, note.text.contains("1.09 loops"))
    }

    @Test
    fun auto_falling_back_on_an_unclear_file_says_so() {
        // The case this whole file exists for. The engine picks the straight cut
        // when it cannot tell, because a wrong cut can be heard and redone while
        // a wrong foldback quietly doubles the tails. That is a decision made
        // for the user, so it is not allowed to be silent.
        val note = Paths.note("auto", plan(detected = "unclear", chosen = "warmup", audibleBars = 11.3))
        assertTrue("a fallback is worth attention", note.warn)
        assertTrue(note.text, note.text.contains("unclear"))
        assertTrue(note.text, note.text.contains("straight cut"))
    }

    @Test
    fun overruling_the_detection_is_allowed_but_not_silent() {
        // Not an error: the user may know something the file does not say. But
        // the likelier cause is a chip left set from the previous file.
        val note = Paths.note("foldback", plan(detected = "warmup", chosen = "foldback", audibleBars = 17.6))
        assertTrue(note.warn)
        assertTrue(note.text, note.text.contains("you chose fold (B)"))
        assertTrue(note.text, note.text.contains("looks like cut (A)"))
    }

    @Test
    fun agreeing_with_the_detection_by_hand_is_not_a_warning() {
        val note = Paths.note("warmup", plan(detected = "warmup", chosen = "warmup", audibleBars = 17.6))
        assertFalse(note.warn)
        assertTrue(note.text, note.text.contains("matches the file"))
    }

    @Test
    fun a_plan_without_a_loop_length_still_produces_a_sentence() {
        // `audibleLoops` is null when the bar count is zero, and a missing
        // number must drop the clause rather than print "NaN loops".
        val note = Paths.note("auto", plan(detected = "warmup", chosen = "warmup", audibleBars = 0.0, bars = 0L))
        assertTrue(note.text, note.text.isNotEmpty())
        assertFalse(note.text, note.text.contains("loops"))
        assertFalse(note.text, note.text.contains("NaN"))
    }
}
