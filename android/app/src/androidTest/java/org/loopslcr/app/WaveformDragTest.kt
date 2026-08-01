package org.loopslcr.app

import androidx.compose.foundation.layout.size
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
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
