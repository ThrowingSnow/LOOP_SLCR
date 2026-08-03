package org.loopslcr.app

import android.graphics.Bitmap
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.onRoot
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import java.io.File

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
        // The tape sliders only exist when the character is on — a panel that
        // renders whether or not it applies is a panel that lies.
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

        // Fold it by its own header, the way a user would.
        compose.onNodeWithText("PLAN  ▴").performClick()
        compose.waitForIdle()

        compose.onNodeWithText("cut").assertDoesNotExist()
        compose.onNodeWithText("clips", substring = true).assertExists()
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
