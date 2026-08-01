package org.loopslcr.app

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.unit.dp
import kotlin.math.abs

/** The two ends of the cut. */
enum class Marker { Start, End }

/**
 * Turning a drag into a pair of bar counts.
 *
 * Pulled out of the view model because it is the only arithmetic in the whole
 * app that is not done in Rust, and arithmetic that cannot be tested is
 * arithmetic that is wrong later. It never produces a *cut point* — only a skip
 * and a bar count, which the exact grid then turns into samples.
 */
object Markers {
    /** Which bar a fraction of the file points at. Never negative. */
    fun barAt(fraction: Float, frames: Long, samplesPerBar: Double): Long {
        if (frames <= 0L || samplesPerBar <= 0.0 || !fraction.isFinite()) return 0L
        val bar = (fraction.toDouble().coerceIn(0.0, 1.0) * frames / samplesPerBar)
        return kotlin.math.round(bar).toLong().coerceAtLeast(0L)
    }

    /** Where that bar sits, as a fraction of the file. The inverse of [barAt]. */
    fun fractionOfBar(bar: Long, frames: Long, samplesPerBar: Double): Float {
        if (frames <= 0L || samplesPerBar <= 0.0) return 0f
        return ((bar * samplesPerBar) / frames).toFloat().coerceIn(0f, 1f)
    }

    /**
     * The new `skip` and `bars` after dragging [marker] to [bar].
     *
     * Dragging the start leaves the end where it is, which is what the gesture
     * looks like it should do. Neither end may cross the other: a loop of zero
     * or negative length is not a smaller loop, it is a nonsense the pipeline
     * would have to refuse further down, where the message means less.
     */
    fun dragged(marker: Marker, bar: Long, skipBars: Long, bars: Long): Pair<Long, Long> =
        when (marker) {
            Marker.Start -> {
                val end = skipBars + bars
                val skip = bar.coerceIn(0L, end - 1)
                skip to (end - skip)
            }
            Marker.End -> skipBars to (bar - skipBars).coerceAtLeast(1L)
        }
}

/**
 * The waveform, drawn from the buckets Rust measured.
 *
 * The buckets arrive as `[c0min, c0max, c1min, c1max, …]` per bucket — one
 * `FloatArray`, walked in the order it is drawn. Measuring happens once per
 * file; a resize only redraws, because the bucket count is fixed and the width
 * is a scale factor, not a new measurement.
 *
 * # Dragging, and the two things that made it feel broken
 *
 * **The gesture must outlive the plan.** `pointerInput` restarts whenever its
 * keys change, and a restart cancels the gesture in progress. Keying it on the
 * region meant that the drag's own effect — a new plan, a moved region — tore
 * down the handler mid-drag, so the finger kept moving and nothing followed. It
 * is keyed on `frames` now, which changes only when a different file is opened,
 * and the current region is read through [rememberUpdatedState] instead.
 *
 * **The line must follow the finger, not the pipeline.** Drawing the marker from
 * the region meant it lagged a debounce plus a native call behind the touch, and
 * rubber-banded when the answer arrived. While a drag is in progress the marker
 * is drawn where the finger is, snapped to the bar it will land on, so what is
 * on screen is a promise rather than a report.
 */
@Composable
fun Waveform(
    peaks: FloatArray,
    channels: Int,
    frames: Long,
    region: LongRange?,
    /** Where the preview is, as a fraction of the whole file, or null when stopped. */
    playHead: Float? = null,
    /** The bar length, so a drag can be shown snapped to the grid it will land on. */
    samplesPerBar: Double? = null,
    /** Called while a marker is dragged, with its new position as a fraction. */
    onDrag: ((Marker, Float) -> Unit)? = null,
    modifier: Modifier = Modifier,
) {
    // Read inside the gesture without keying the handler on it: the region moves
    // *because* of the drag, and a handler that restarted on its own effect
    // cancelled every gesture that mattered.
    val current by rememberUpdatedState(region)

    // Which marker the finger grabbed, decided once when the drag starts.
    // Re-deciding per movement would let a fast drag hand the gesture to the
    // other marker halfway through and swap the ends of the loop.
    var grabbed by remember { mutableStateOf<Marker?>(null) }
    var at by remember { mutableFloatStateOf(0f) }

    val gestures = if (onDrag == null || frames <= 0L) {
        Modifier
    } else {
        Modifier.pointerInput(frames) {
            detectDragGestures(
                onDragStart = { down ->
                    val here = current
                    if (here != null) {
                        val start = size.width * (here.first.toFloat() / frames)
                        val end = size.width * (here.last.toFloat() / frames)
                        grabbed = if (abs(down.x - start) <= abs(down.x - end)) {
                            Marker.Start
                        } else {
                            Marker.End
                        }
                        at = (down.x / size.width).coerceIn(0f, 1f)
                    }
                },
                onDrag = { change, _ ->
                    change.consume()
                    at = (change.position.x / size.width).coerceIn(0f, 1f)
                    grabbed?.let { onDrag(it, at) }
                },
                onDragEnd = { grabbed = null },
                onDragCancel = { grabbed = null },
            )
        }
    }

    Canvas(modifier.then(gestures)) {
        val buckets = if (channels > 0) peaks.size / (channels * 2) else 0
        if (buckets == 0) return@Canvas

        drawRect(Palette.waveBackground)

        // Where the drag will land, snapped, or null when nothing is being
        // dragged. Snapped here as well as in the view model — through the same
        // `Markers.barAt` — so the line cannot promise a position the commit
        // then rounds somewhere else.
        val held = grabbed
        val preview = if (held != null && samplesPerBar != null && samplesPerBar > 0.0) {
            Markers.fractionOfBar(Markers.barAt(at, frames, samplesPerBar), frames, samplesPerBar)
        } else if (held != null) {
            at
        } else {
            null
        }

        val left: Float?
        val right: Float?
        if (region != null && frames > 0L) {
            val fromRegion = region.first.toFloat() / frames
            val toRegion = region.last.toFloat() / frames
            left = if (held == Marker.Start && preview != null) preview else fromRegion
            right = if (held == Marker.End && preview != null) preview else toRegion
        } else {
            left = null
            right = null
        }

        val laneHeight = size.height / channels
        for (channel in 0 until channels) {
            val top = laneHeight * channel
            val mid = top + laneHeight / 2f
            val scale = laneHeight / 2f

            drawLine(Palette.axis, Offset(0f, mid), Offset(size.width, mid), strokeWidth = 1f)

            val step = size.width / buckets
            for (i in 0 until buckets) {
                val base = (i * channels + channel) * 2
                val low = peaks[base].coerceIn(-1f, 1f)
                val high = peaks[base + 1].coerceIn(-1f, 1f)
                val x = i * step
                // A silent bucket would otherwise draw nothing at all, and a gap
                // in the line reads as missing data rather than as silence.
                val yTop = mid - high * scale
                val yBottom = mid - low * scale
                drawRect(
                    color = Palette.wave,
                    topLeft = Offset(x, yTop),
                    size = Size(maxOf(step, 1f), maxOf(yBottom - yTop, 1f)),
                )
            }
        }

        // Everything outside the cut is dimmed rather than hidden: the user is
        // choosing a region *of a file*, and a file whose discarded parts have
        // vanished gives them nothing to judge the choice against.
        //
        // **After** the waveform, which is where this was wrong from the first
        // version: drawn before it, the bars painted straight over the shading
        // and the region was invisible. It never showed on the emulator because
        // there the cut was always the whole file, so there was nothing outside
        // it to dim — a bug that only a real file could reveal.
        if (left != null && right != null) {
            drawRect(Palette.outside, Offset(0f, 0f), Size(size.width * left, size.height))
            val end = size.width * right
            drawRect(Palette.outside, Offset(end, 0f), Size(size.width - end, size.height))

            marker(size.width * left, held == Marker.Start, atStart = true)
            marker(size.width * right, held == Marker.End, atStart = false)
        }

        if (playHead != null) {
            val x = size.width * playHead.coerceIn(0f, 1f)
            drawRect(Palette.playHead, Offset(x - 1f, 0f), Size(3f, size.height))
        }
    }
}

/**
 * One end of the cut, with a handle.
 *
 * A two-pixel line is a thing to look at, not a thing to grab. The handle is the
 * affordance: it sits inside the loop so the two never overlap at a short cut,
 * and it brightens while held so the finger knows which one it took.
 */
private fun DrawScope.marker(x: Float, held: Boolean, atStart: Boolean) {
    // Not the play head's green: both can be on screen at once, and two things
    // that mean different things must not look the same.
    val colour = if (held) Palette.grabbed else Palette.marker
    val line = if (held) 3f else 2f
    drawRect(colour, Offset(x - line / 2f, 0f), Size(line, size.height))

    // Sized in dp, not pixels. Sixteen pixels is a millimetre and a half on a
    // phone — a mark, not a target.
    val width = 12.dp.toPx()
    val height = 18.dp.toPx()
    val left = if (atStart) x else x - width
    drawRect(colour, Offset(left, 0f), Size(width, height))
}

/** The one place a colour is chosen. */
object Palette {
    val background = Color(0xFF101014)
    val surface = Color(0xFF17171D)
    val waveBackground = Color(0xFF0B0B0E)
    val wave = Color(0xFFFF6B4A)
    val outside = Color(0x99000000)
    val axis = Color(0xFF2A2A33)
    val marker = Color(0xFFF2F2F2)
    /** A marker under a finger. Deliberately not the play head's colour. */
    val grabbed = Color(0xFF7DD3FC)
    val playHead = Color(0xFF4ADE80)
    val text = Color(0xFFE8E8EC)
    val dim = Color(0xFF8A8A96)
    val warn = Color(0xFFFFB020)
    val bad = Color(0xFFFF4D4D)
}
