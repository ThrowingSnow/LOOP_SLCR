package org.loopslcr.app

import android.graphics.Bitmap
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.onRoot
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import java.io.File
import kotlin.math.absoluteValue

/**
 * The screen with a file in it.
 *
 * The engine tests prove the audio; this proves the layout that shows it —
 * including the waveform canvas, which draws from a `FloatArray` and would fail
 * on a length mismatch that no type would have caught.
 *
 * It also leaves a screenshot behind. Not an assertion: a way to look at what
 * the thing renders without an emulator on screen.
 */
class CutterScreenTest {
    @get:Rule
    val compose = createComposeRule()

    private val name = "103 loop.wav"

    @Test
    fun a_loaded_file_renders_with_its_waveform_and_its_numbers() {
        val raw = EngineTest.wav(bars = 8)
        val analysis = Engine.analyze(org.loopslcr.Native.direct(raw), "200 loop.wav")
        val peaks = Engine.peaks(org.loopslcr.Native.direct(raw), 512)
        // A partial cut, so the screenshot shows a region rather than the whole file.
        val plan = Engine.plan(org.loopslcr.Native.direct(raw), "200 loop.wav", Settings(bars = 4L, skip = 2L))

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                CutterScreen(
                    loaded = Loaded(name, org.loopslcr.Native.direct(raw), analysis, peaks),
                    settings = Settings(tape = true, bars = 4L, skip = 2L),
                    plan = plan,
                    busy = Busy.Idle,
                    problem = null,
                    onOpen = {},
                    onExport = {},
                    onChange = {},
                    onDismissProblem = {},
                )
            }
        }

        compose.onNodeWithText(name).assertExists()
        compose.onNodeWithText("Export").assertExists()

        // Groups start folded, so the tape panel is not composed until its
        // header is tapped — and then it is there. The tape sliders exist only
        // when the character is on: a panel that renders whether or not it
        // applies is a panel that lies.
        compose.onNodeWithText("Tape character").assertDoesNotExist()
        compose.onNodeWithText("TAPE").performClick()
        compose.waitForIdle()
        compose.onNodeWithText("Tape character").assertExists()

        val shot = compose.onRoot().captureToImage()
        assertTrue(shot.width > 0 && shot.height > 0)
        save(shot.asAndroidBitmap(), "cutter.png")
    }

    @Test
    fun the_file_figures_hide_behind_the_name_until_it_is_tapped() {
        // They used to sit between the waveform and every control, so the first
        // thing you could actually change was a scroll away. Behind the name
        // they are one tap off — but the tap has to work, and the figures must
        // not be on screen before it.
        val raw = EngineTest.wav(bars = 8)
        val analysis = Engine.analyze(org.loopslcr.Native.direct(raw), "200 loop.wav")
        val peaks = Engine.peaks(org.loopslcr.Native.direct(raw), 512)

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                CutterScreen(
                    loaded = Loaded(name, org.loopslcr.Native.direct(raw), analysis, peaks),
                    settings = Settings(),
                    plan = null,
                    busy = Busy.Idle,
                    problem = null,
                    onOpen = {},
                    onExport = {},
                    onChange = {},
                    onDismissProblem = {},
                )
            }
        }

        compose.onNodeWithText("format").assertDoesNotExist()
        compose.onNodeWithText(name).performClick()
        compose.waitForIdle()
        compose.onNodeWithText("format").assertExists()
        compose.onNodeWithText("peak").assertExists()

        // And it closes again, or it is a one-way door rather than a drawer.
        compose.onNodeWithText(name).performClick()
        compose.waitForIdle()
        compose.onNodeWithText("format").assertDoesNotExist()
    }

    @Test
    fun play_holds_the_corner_and_open_waits_behind_the_name() {
        // The corner belongs to whatever is pressed most. A file is chosen once
        // and then listened to for minutes, so Play took it — and Open, which
        // throws away every setting on the screen, moved somewhere deliberate.
        val raw = EngineTest.wav(bars = 8)
        val analysis = Engine.analyze(org.loopslcr.Native.direct(raw), "200 loop.wav")
        val peaks = Engine.peaks(org.loopslcr.Native.direct(raw), 512)

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                CutterScreen(
                    loaded = Loaded(name, org.loopslcr.Native.direct(raw), analysis, peaks),
                    settings = Settings(),
                    plan = null,
                    busy = Busy.Idle,
                    problem = null,
                    onOpen = {},
                    onExport = {},
                    onChange = {},
                    onDismissProblem = {},
                )
            }
        }

        compose.onNodeWithText("Play").assertExists()
        compose.onNodeWithText("Open another file").assertDoesNotExist()

        compose.onNodeWithText(name).performClick()
        compose.waitForIdle()
        compose.onNodeWithText("Open another file").assertExists()
    }

    @Test
    fun a_folded_plan_card_still_says_that_the_cut_clips() {
        // The rule that makes folding safe at all: detail may hide, trouble may
        // not. A card that could swallow "clips" would be worse than one that
        // does not fold, because the number it hid was the reason to look.
        val raw = EngineTest.wav(bars = 8)
        val analysis = Engine.analyze(org.loopslcr.Native.direct(raw), "200 loop.wav")
        val peaks = Engine.peaks(org.loopslcr.Native.direct(raw), 512)
        val plan = Engine.plan(org.loopslcr.Native.direct(raw), "200 loop.wav", Settings())
            .copy(clips = true, peak = 1.4998)

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                CutterScreen(
                    loaded = Loaded(name, org.loopslcr.Native.direct(raw), analysis, peaks),
                    settings = Settings(),
                    plan = plan,
                    busy = Busy.Idle,
                    problem = null,
                    onOpen = {},
                    onExport = {},
                    onChange = {},
                    onDismissProblem = {},
                )
            }
        }

        // Everything starts folded now, so this is the state the user meets.
        compose.onNodeWithText("cut").assertDoesNotExist()
        compose.onNodeWithText("clips", substring = true).assertExists()

        // And unfolding still gives the detail back.
        compose.onNodeWithText("clips", substring = true).performClick()
        compose.waitForIdle()
        compose.onNodeWithText("cut").assertExists()
    }

    @Test
    fun the_pitch_slider_sits_under_the_waveform_not_in_a_folded_group() {
        // It is the one control held *while listening*, so it has to be within
        // sight of the picture it acts on. Checked by geometry rather than by
        // reading the source: "under the waveform" is a claim about pixels.
        val raw = EngineTest.wav(bars = 8)
        val analysis = Engine.analyze(org.loopslcr.Native.direct(raw), "200 loop.wav")
        val peaks = Engine.peaks(org.loopslcr.Native.direct(raw), 512)
        val plan = Engine.plan(org.loopslcr.Native.direct(raw), "200 loop.wav", Settings())

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                CutterScreen(
                    loaded = Loaded(name, org.loopslcr.Native.direct(raw), analysis, peaks),
                    settings = Settings(semitones = -3.0),
                    plan = plan,
                    busy = Busy.Idle,
                    problem = null,
                    onOpen = {},
                    onExport = {},
                    onChange = {},
                    onDismissProblem = {},
                )
            }
        }

        // The claim is "reachable while watching the waveform", so the check is
        // that it is in the viewport *without scrolling* — together with the
        // line that sits directly under the waveform. Comparing raw bounds
        // against the VARISPEED header does not work: a node scrolled out of
        // view reports zero, which would pass for the wrong reason.
        compose.onNodeWithText("speed").assertIsDisplayed()
        compose.onNodeWithText("drag a marker", substring = true).assertIsDisplayed()

        val pitch = compose.onNodeWithText("speed").getBoundsInRoot()
        val hint = compose.onNodeWithText("drag a marker", substring = true).getBoundsInRoot()
        assertTrue("pitch at ${pitch.top}, hint at ${hint.top}", pitch.top > hint.top)

        compose.onNodeWithText("-3.00 st", substring = true).assertExists()
    }

    @Test
    fun a_tempo_arriving_from_elsewhere_reaches_the_field() {
        // The bug behind "send the BPM from the calculator and it just adapts".
        // The field kept its text in a `remember` keyed on the *mode*, so a
        // tempo that arrived while target-BPM mode was already selected left the
        // old number sitting there — which looks exactly like nothing happened.
        val raw = EngineTest.wav(bars = 8)
        val analysis = Engine.analyze(org.loopslcr.Native.direct(raw), "200 loop.wav")
        val peaks = Engine.peaks(org.loopslcr.Native.direct(raw), 512)
        val plan = Engine.plan(org.loopslcr.Native.direct(raw), "200 loop.wav", Settings())

        // Already in target-BPM mode, as it would be on a second send.
        var settings by mutableStateOf(
            Settings(speedMode = SpeedMode.TargetBpm, targetBpm = 150.0),
        )

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                CutterScreen(
                    loaded = Loaded(name, org.loopslcr.Native.direct(raw), analysis, peaks),
                    settings = settings,
                    plan = plan,
                    busy = Busy.Idle,
                    problem = null,
                    onOpen = {},
                    onExport = {},
                    onChange = { change -> settings = change(settings) },
                    onDismissProblem = {},
                )
            }
        }

        compose.onNodeWithText("150").assertExists()

        // What `sendTempoToCutter` does, from outside the screen.
        settings = settings.copy(speedMode = SpeedMode.TargetBpm, targetBpm = 90.0)
        compose.waitForIdle()

        compose.onNodeWithText("90").assertExists()
        compose.onNodeWithText("150").assertDoesNotExist()
    }

    @Test
    fun the_typed_tempo_rides_the_row_of_the_speed_it_sets() {
        // It was a full-width Material field under the slider: a floating label,
        // a 56 dp minimum and most of a thumb's height of air around four
        // characters — on the one screen whose entire layout exists so that the
        // waveform never has to move. The number you type and the number it
        // produces are one fact, so they share a line.
        val raw = EngineTest.wav(bars = 8)
        val analysis = Engine.analyze(org.loopslcr.Native.direct(raw), "200 loop.wav")
        val peaks = Engine.peaks(org.loopslcr.Native.direct(raw), 512)
        val plan = Engine.plan(org.loopslcr.Native.direct(raw), "200 loop.wav", Settings())

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                CutterScreen(
                    loaded = Loaded(name, org.loopslcr.Native.direct(raw), analysis, peaks),
                    settings = Settings(speedMode = SpeedMode.TargetBpm, targetBpm = 150.0),
                    plan = plan,
                    busy = Busy.Idle,
                    problem = null,
                    onOpen = {},
                    onExport = {},
                    onChange = {},
                    onDismissProblem = {},
                )
            }
        }

        val field = compose.onNodeWithTag("tempoField").getBoundsInRoot()
        val speed = compose.onNodeWithText("speed").getBoundsInRoot()

        // Same line: the two boxes overlap vertically. Nothing that sits under
        // the slider can satisfy this.
        assertTrue(
            "field ${field.top}..${field.bottom}, speed ${speed.top}..${speed.bottom}",
            field.top < speed.bottom && speed.top < field.bottom,
        )

        // A quarter of the strip it sits in — measured against the strip, not
        // against what the readout left over. A field sized by the leftovers
        // changes width as the readout does, which is every drag of the slider.
        val strip = compose.onNodeWithTag("varispeed").getBoundsInRoot()
        val width = field.right - field.left
        val quarter = (strip.right - strip.left) / 4
        assertTrue(
            "field is $width, a quarter of the strip is $quarter",
            (width - quarter).value.absoluteValue < 2f,
        )

        // And it is at the right-hand end of that strip, not floating mid-row.
        assertTrue(
            "field ends at ${field.right}, the strip at ${strip.right}",
            (strip.right - field.right).value.absoluteValue < 2f,
        )
    }

    @Test
    fun the_target_bpm_slider_is_there_even_before_the_first_plan() {
        // Reported as "I switch to BPM and there is no slider". It hung on
        // `plan?.tempo`, and the plan is null before the first one lands and
        // again whenever the settings do not describe a cut — so the control
        // vanished, with nothing said. The file's own tempo is a perfectly good
        // answer and was sitting right there.
        val raw = EngineTest.wav(bars = 8)
        val analysis = Engine.analyze(org.loopslcr.Native.direct(raw), "200 loop.wav")
        val peaks = Engine.peaks(org.loopslcr.Native.direct(raw), 512)

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                CutterScreen(
                    loaded = Loaded(name, org.loopslcr.Native.direct(raw), analysis, peaks),
                    settings = Settings(speedMode = SpeedMode.TargetBpm),
                    plan = null,
                    busy = Busy.Idle,
                    problem = null,
                    onOpen = {},
                    onExport = {},
                    onChange = {},
                    onDismissProblem = {},
                )
            }
        }

        // 200 BPM from the name, so the span is 100..400 and the slider stands.
        compose.onNodeWithTag("bpmSlider").assertExists()
        compose.onNodeWithText("no source tempo yet", substring = true).assertDoesNotExist()
    }

    @Test
    fun the_waveform_and_the_varispeed_do_not_scroll_away() {
        // The whole screen used to scroll as one, so reaching any control
        // pushed the picture it acted on off the top: you could change the
        // thing or watch it, never both.
        val raw = EngineTest.wav(bars = 8)
        val analysis = Engine.analyze(org.loopslcr.Native.direct(raw), "200 loop.wav")
        val peaks = Engine.peaks(org.loopslcr.Native.direct(raw), 512)
        val plan = Engine.plan(org.loopslcr.Native.direct(raw), "200 loop.wav", Settings())

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                CutterScreen(
                    loaded = Loaded(name, org.loopslcr.Native.direct(raw), analysis, peaks),
                    settings = Settings(),
                    plan = plan,
                    busy = Busy.Idle,
                    problem = null,
                    onOpen = {},
                    onExport = {},
                    onChange = {},
                    onDismissProblem = {},
                )
            }
        }

        // Open every group first. With everything folded the screen fits on an
        // emulator, so there is nothing to scroll and the test cannot tell the
        // layouts apart — it passed against the old one until this was added.
        for (group in listOf("SOURCE", "LOOP", "TAPE", "OUTPUT")) {
            compose.onNodeWithText(group).performClick()
            compose.waitForIdle()
        }

        compose.onNodeWithText("speed").assertIsDisplayed()
        val before = compose.onNodeWithText("speed").getBoundsInRoot()

        // Scroll the lower half to its end, which is as far as it can go.
        compose.onNodeWithText("Export").performScrollTo()
        compose.waitForIdle()

        compose.onNodeWithText("speed").assertIsDisplayed()
        assertEquals(before.top, compose.onNodeWithText("speed").getBoundsInRoot().top)
    }

    @Test
    fun the_calculator_renders_its_table() {
        val settings = CalculatorSettings(bpm = "103", sampleRate = 44_100, bars = 8)
        val sums = Calculator.compute(settings).getOrThrow()

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                CalculatorScreen(settings = settings, sums = sums, problem = null, onChange = {})
            }
        }

        // Not "1/4": that is also a BPM-unit chip, and a selector that matches
        // two different things is a selector that will pass for the wrong reason.
        compose.onNodeWithText("1/16T").assertExists()
        compose.onNodeWithText("NOTE VALUES").assertExists()
        // 103 BPM divides no sample rate cleanly, so every row should be marked
        // inexact — the one fact on the screen that is not decoration.
        assertTrue(sums.notes.none { it.sampleExact })

        save(compose.onRoot().captureToImage().asAndroidBitmap(), "calculator.png")
    }

    private fun save(bitmap: Bitmap, name: String) {
        val dir = InstrumentationRegistry.getInstrumentation().targetContext.filesDir
        File(dir, name).outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }
}
