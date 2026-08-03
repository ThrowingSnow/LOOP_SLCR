package org.loopslcr.app

import androidx.compose.foundation.layout.size
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.unit.dp
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

/**
 * The drag, and the bug that made it feel broken.
 *
 * Dragging a marker moves the region, which is exactly the state the gesture
 * handler was keyed on — so the drag's own effect restarted the handler and
 * cancelled the drag. The finger kept moving and nothing followed.
 *
 * The test reproduces that shape directly: the region is changed *from inside*
 * the drag callback, as the real plan does, and the gesture has to survive it.
 */
class WaveformDragTest {
    @get:Rule
    val compose = createComposeRule()

    private val frames = 76_800L
    private val perBar = 9_600.0
    private val peaks = FloatArray(64 * 2 * 2) { if (it % 2 == 0) -0.5f else 0.5f }

    @Test
    fun a_drag_survives_the_region_moving_underneath_it() {
        var region by mutableStateOf(0L..(8 * perBar).toLong())
        val moves = mutableListOf<Float>()

        compose.setContent {
            Waveform(
                peaks = peaks,
                channels = 2,
                frames = frames,
                region = region,
                samplesPerBar = perBar,
                onDrag = { marker, at ->
                    moves += at
                    // What the view model does: the drag changes the plan, and
                    // the plan is where the region comes from.
                    val (skip, bars) = Markers.dragged(
                        marker,
                        Markers.barAt(at, frames, perBar),
                        region.first / perBar.toLong(),
                        (region.last - region.first) / perBar.toLong(),
                    )
                    region = (skip * perBar.toLong())..((skip + bars) * perBar.toLong())
                },
                modifier = Modifier.testTag("wave").size(320.dp, 120.dp),
            )
        }

        // One injected block per movement, with a recomposition between them.
        // A single block dispatches the whole gesture before Compose has redrawn
        // anything, so the handler never sees the state it broke on — the test
        // passed against the bug until it was split like this.
        val wave = compose.onNodeWithTag("wave")
        wave.performTouchInput { down(centerLeft) }
        compose.waitForIdle()
        wave.performTouchInput { moveTo(centerLeft + (center - centerLeft) / 2f) }
        compose.waitForIdle()
        wave.performTouchInput { moveTo(center) }
        compose.waitForIdle()
        wave.performTouchInput { moveTo(center + (centerRight - center) / 2f) }
        compose.waitForIdle()
        wave.performTouchInput { up() }
        compose.waitForIdle()

        // Before the fix this collected one move and then the gesture was torn
        // down. Four movements were made; every one of them has to arrive.
        assertTrue("only ${moves.size} move(s) arrived", moves.size >= 3)
        assertTrue("the drag never reached the right half: $moves", moves.any { it > 0.6f })
    }

    @Test
    fun what_lies_outside_the_cut_is_visibly_dimmed() {
        // This is the bug a screenshot could not catch. The shading was drawn
        // before the waveform, so the bars painted straight over it — and every
        // emulator test had the whole file as its region, leaving nothing
        // outside to dim. Only a real file with a partial cut showed it.
        //
        // So it is checked in pixels: a column outside the region must be
        // darker than a column inside it.
        compose.setContent {
            Waveform(
                peaks = FloatArray(64 * 2 * 2) { if (it % 2 == 0) -1f else 1f },
                channels = 2,
                frames = frames,
                region = (frames / 4)..(frames * 3 / 4),
                modifier = Modifier.testTag("wave").size(320.dp, 120.dp),
            )
        }

        val shot = compose.onNodeWithTag("wave").captureToImage().asAndroidBitmap()
        val inside = brightness(shot, shot.width / 2)
        val before = brightness(shot, shot.width / 8)
        val after = brightness(shot, shot.width * 7 / 8)

        assertTrue("before the cut: $before vs inside $inside", before < inside * 0.8)
        assertTrue("after the cut: $after vs inside $inside", after < inside * 0.8)
    }

    @Test
    fun the_bar_grid_stands_where_the_bars_are() {
        // A grid that is off by a bar is worse than no grid: it is a picture of
        // a tempo the file does not have, and every judgement made by eye from
        // then on is wrong. So the lines are checked against the arithmetic that
        // put them there rather than against a screenshot.
        //
        // A silent file, so nothing but the grid is drawn and a bar line is the
        // only thing that can be brighter than the background.
        val bars = 8
        val perBarHere = frames.toDouble() / bars
        compose.setContent {
            Waveform(
                peaks = FloatArray(64 * 2 * 2),
                channels = 2,
                frames = frames,
                region = null,
                samplesPerBar = perBarHere,
                modifier = Modifier.testTag("wave").size(320.dp, 120.dp),
            )
        }

        val shot = compose.onNodeWithTag("wave").captureToImage().asAndroidBitmap()
        val step = shot.width.toDouble() / bars

        // On a line, and a third of a bar away from one. The first must be
        // brighter; nothing else in the picture is.
        for (bar in 1 until bars) {
            val on = brightness(shot, (step * bar).toInt().coerceIn(0, shot.width - 1))
            val off = brightness(shot, (step * (bar + 0.33)).toInt().coerceIn(0, shot.width - 1))
            assertTrue("bar $bar: line $on vs gap $off", on > off)
        }
    }

    @Test
    fun a_grid_too_fine_to_read_is_not_drawn_at_all() {
        // Below about four pixels apart the lines stop being a grid and become a
        // wash, and a wash reads as part of the signal — a quiet passage would
        // look busier than it is. Nothing beats a tint that misinforms.
        compose.setContent {
            Waveform(
                peaks = FloatArray(64 * 2 * 2),
                channels = 2,
                frames = frames,
                region = null,
                // Two thousand bars across 320 dp: far under a pixel each.
                samplesPerBar = frames.toDouble() / 2000.0,
                modifier = Modifier.testTag("wave").size(320.dp, 120.dp),
            )
        }

        val shot = compose.onNodeWithTag("wave").captureToImage().asAndroidBitmap()
        val columns = (1 until shot.width).map { brightness(shot, it) }
        val darkest = columns.min()
        val brightest = columns.max()
        assertTrue(
            "the picture is not uniform: $darkest to $brightest",
            brightest - darkest < 1.0,
        )
    }

    /** Mean luminance of one column, as a stand-in for "how bright is this bit". */
    private fun brightness(bitmap: android.graphics.Bitmap, x: Int): Double {
        var total = 0.0
        for (y in 0 until bitmap.height) {
            val p = bitmap.getPixel(x, y)
            total += ((p shr 16 and 0xFF) + (p shr 8 and 0xFF) + (p and 0xFF)) / 3.0
        }
        return total / bitmap.height
    }

    @Test
    fun the_snapped_preview_and_the_committed_bar_are_the_same_place() {
        // The line drawn under the finger and the bar the drag commits to come
        // from one function each way round; if they disagreed the marker would
        // jump on release.
        for (at in listOf(0f, 0.13f, 0.49f, 0.5f, 0.87f, 1f)) {
            val bar = Markers.barAt(at, frames, perBar)
            val drawn = Markers.fractionOfBar(bar, frames, perBar)
            assertEquals(bar, Markers.barAt(drawn, frames, perBar))
        }
    }
}
