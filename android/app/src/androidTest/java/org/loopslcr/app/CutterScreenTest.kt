package org.loopslcr.app

import android.graphics.Bitmap
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
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
        val wav = EngineTest.wav(bars = 8)
        val analysis = Engine.analyze(wav, "200 loop.wav")
        val peaks = Engine.peaks(wav, 512)
        val plan = Engine.plan(wav, "200 loop.wav", Settings())

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                CutterScreen(
                    loaded = Loaded(name, wav, analysis, peaks),
                    settings = Settings(tape = true),
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
