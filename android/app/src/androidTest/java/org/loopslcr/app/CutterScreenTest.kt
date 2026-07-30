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

    private fun save(bitmap: Bitmap, name: String) {
        val dir = InstrumentationRegistry.getInstrumentation().targetContext.filesDir
        File(dir, name).outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }
}
