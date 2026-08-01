package org.loopslcr.app

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * The one piece of arithmetic in this app that is not done in Rust.
 *
 * It maps a fingertip to a bar index, and never to a cut point — so the worst it
 * can do is pick the wrong bar, not produce a loop that drifts. That is still
 * worth testing, because "the worst it can do" is the kind of claim that stops
 * being true quietly.
 */
class MarkersTest {
    private val frames = 76_800L
    private val perBar = 9_600.0

    @Test
    fun a_fraction_maps_to_the_bar_under_it() {
        assertEquals(0L, Markers.barAt(0f, frames, perBar))
        assertEquals(4L, Markers.barAt(0.5f, frames, perBar))
        assertEquals(8L, Markers.barAt(1f, frames, perBar))
    }

    @Test
    fun a_fraction_between_bars_goes_to_the_nearer_one() {
        // 0.45 of the file is bar 3.6, which is bar 4 — snapping to the nearest
        // line, not truncating towards the start.
        assertEquals(4L, Markers.barAt(0.45f, frames, perBar))
        assertEquals(3L, Markers.barAt(0.42f, frames, perBar))
    }

    @Test
    fun nonsense_produces_the_start_rather_than_a_crash() {
        assertEquals(0L, Markers.barAt(Float.NaN, frames, perBar))
        assertEquals(0L, Markers.barAt(0.5f, 0L, perBar))
        assertEquals(0L, Markers.barAt(0.5f, frames, 0.0))
        assertEquals(0L, Markers.barAt(-3f, frames, perBar))
    }

    @Test
    fun dragging_the_start_holds_the_end_still() {
        // Bars 2..10 — dragging the start to bar 6 must leave the end at 10.
        val (skip, bars) = Markers.dragged(Marker.Start, bar = 6, skipBars = 2, bars = 8)
        assertEquals(6L, skip)
        assertEquals(4L, bars)
        assertEquals(10L, skip + bars)
    }

    @Test
    fun dragging_the_end_holds_the_start_still() {
        val (skip, bars) = Markers.dragged(Marker.End, bar = 6, skipBars = 2, bars = 8)
        assertEquals(2L, skip)
        assertEquals(4L, bars)
    }

    @Test
    fun neither_end_may_cross_the_other() {
        // A loop of zero or negative length is not a smaller loop.
        val (skip, bars) = Markers.dragged(Marker.Start, bar = 99, skipBars = 2, bars = 8)
        assertEquals(9L, skip)
        assertEquals(1L, bars)

        val (skip2, bars2) = Markers.dragged(Marker.End, bar = 0, skipBars = 2, bars = 8)
        assertEquals(2L, skip2)
        assertEquals(1L, bars2)
    }

    @Test
    fun dragging_the_start_before_the_file_stops_at_zero() {
        val (skip, bars) = Markers.dragged(Marker.Start, bar = 0, skipBars = 4, bars = 4)
        assertEquals(0L, skip)
        assertEquals(8L, bars)
    }
}
