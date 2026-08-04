package org.loopslcr.app

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.drag
import androidx.compose.foundation.gestures.detectTransformGestures
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
    var grabbed by remember { mutableStateOf<Marker?>(null) }
    var at by remember { mutableFloatStateOf(0f) }

    // The visible window: a magnification, and a left edge in file fractions.
    // Keyed on the file — a window from the last one means nothing here.
    var zoom by remember(frames) { mutableFloatStateOf(1f) }
    var left by remember(frames) { mutableFloatStateOf(0f) }

    Canvas(
        modifier
            // Two fingers to zoom; one to pan, once there is somewhere to pan to.
            .pointerInput(frames) {
                if (frames <= 0L) return@pointerInput
                detectTransformGestures { centroid, pan, gestureZoom, _ ->
                    if (grabbed != null) return@detectTransformGestures
                    val width = size.width.toFloat()
                    if (width <= 0f) return@detectTransformGestures

                    // Zoom about the centroid, so the audio under the fingers
                    // stays under the fingers. Anchoring to the left edge makes
                    // a pinch feel like it is fighting you.
                    val under = left + (centroid.x / width) / zoom
                    zoom = (zoom * gestureZoom).coerceIn(1f, MAX_ZOOM)
                    left = under - (centroid.x / width) / zoom - (pan.x / width) / zoom
                    left = left.coerceIn(0f, (1f - 1f / zoom).coerceAtLeast(0f))
                }
            }
            // Markers *last* in the chain, which means first to see a touch:
            // pointer events reach the innermost handler first, and the zoom
            // detector consumes anything past the touch slop.
            //
            // Written out with `awaitEachGesture` rather than with
            // `detectDragGestures`, because the hit test has to run on the point
            // the finger actually landed on. `onDragStart` reports the position
            // *after* the touch slop is crossed — tens of pixels away — so a
            // handle grab was tested against somewhere the user never touched,
            // and both the grabbing and the not-grabbing came out wrong.
            //
            // It engages only on a handle. The body of the waveform belongs to
            // the zoom: a drag anywhere on it used to take whichever end was
            // nearer, which is how a pinch would fling the cut across the file
            // before the second finger had landed.
            .pointerInput(frames, onDrag) {
                if (onDrag == null || frames <= 0L) return@pointerInput
                awaitEachGesture {
                    val down = awaitFirstDown(requireUnconsumed = false)
                    val here = current ?: return@awaitEachGesture
                    val width = size.width.toFloat()
                    if (width <= 0f) return@awaitEachGesture

                    val reach = HANDLE_TOUCH_DP.dp.toPx()
                    val startX = (here.first.toFloat() / frames - left) * zoom * width
                    val endX = (here.last.toFloat() / frames - left) * zoom * width
                    val took = when {
                        abs(down.position.x - startX) <= reach -> Marker.Start
                        abs(down.position.x - endX) <= reach -> Marker.End
                        else -> null
                    } ?: return@awaitEachGesture

                    grabbed = took
                    at = (left + (down.position.x / width) / zoom).coerceIn(0f, 1f)
                    down.consume()

                    drag(down.id) { change ->
                        change.consume()
                        at = (left + (change.position.x / width) / zoom).coerceIn(0f, 1f)
                        onDrag(took, at)
                    }
                    grabbed = null
                }
            },
    ) {
        val buckets = if (channels > 0) peaks.size / (channels * 2) else 0
        if (buckets == 0) return@Canvas

        drawRect(Palette.waveBackground)

        // File fraction to screen. Everything on this canvas goes through it, so
        // the grid, the markers, the shading and the play head cannot disagree
        // about where the view is.
        fun x(fraction: Float) = (fraction - left) * zoom * size.width

        val held = grabbed
        val preview = if (held != null && samplesPerBar != null && samplesPerBar > 0.0) {
            Markers.fractionOfBar(Markers.barAt(at, frames, samplesPerBar), frames, samplesPerBar)
        } else if (held != null) {
            at
        } else {
            null
        }

        val from: Float?
        val to: Float?
        if (region != null && frames > 0L) {
            val fromRegion = region.first.toFloat() / frames
            val toRegion = region.last.toFloat() / frames
            from = if (held == Marker.Start && preview != null) preview else fromRegion
            to = if (held == Marker.End && preview != null) preview else toRegion
        } else {
            from = null
            to = null
        }

        gridLines(frames, samplesPerBar, left, zoom)

        val laneHeight = size.height / channels
        for (channel in 0 until channels) {
            val top = laneHeight * channel
            val mid = top + laneHeight / 2f
            val scale = laneHeight / 2f

            drawLine(Palette.axis, Offset(0f, mid), Offset(size.width, mid), strokeWidth = 1f)

            // Only the buckets inside the window, spread across the full width.
            // Measuring once at a resolution the zoom can spend is what makes
            // this instant: nothing to fetch, nothing to wait for.
            val step = size.width * zoom / buckets
            val firstBucket = (left * buckets).toInt().coerceIn(0, buckets - 1)
            val lastBucket = ((left + 1f / zoom) * buckets).toInt().coerceIn(0, buckets - 1)
            for (i in firstBucket..lastBucket) {
                val base = (i * channels + channel) * 2
                val low = peaks[base].coerceIn(-1f, 1f)
                val high = peaks[base + 1].coerceIn(-1f, 1f)
                val px = x(i.toFloat() / buckets)
                // A silent bucket would otherwise draw nothing at all, and a gap
                // in the line reads as missing data rather than as silence.
                val yTop = mid - high * scale
                val yBottom = mid - low * scale
                drawRect(
                    color = Palette.wave,
                    topLeft = Offset(px, yTop),
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
        // and the region was invisible.
        if (from != null && to != null) {
            val startX = x(from)
            val endX = x(to)
            if (startX > 0f) {
                drawRect(Palette.outside, Offset(0f, 0f), Size(startX, size.height))
            }
            if (endX < size.width) {
                drawRect(Palette.outside, Offset(endX, 0f), Size(size.width - endX, size.height))
            }

            marker(startX, held == Marker.Start, atStart = true)
            marker(endX, held == Marker.End, atStart = false)
        }

        if (playHead != null) {
            val px = x(playHead.coerceIn(0f, 1f))
            drawRect(Palette.playHead, Offset(px - 1f, 0f), Size(3f, size.height))
        }
    }
}

/** How far from a marker's line a finger still counts as having taken it. */
private const val HANDLE_TOUCH_DP = 22f

/**
 * How far in the view can go.
 *
 * Bounded by the measurement, not by taste. The buckets are measured once when
 * the file opens and the zoom spends them: at [MAX_ZOOM] the window holds an
 * eighth of them, which is the same density the whole file is drawn at
 * unzoomed. Going further would not show more — it would show the same data
 * drawn wider, a magnified claim rather than a closer look.
 */
const val MAX_ZOOM = 8f


/**
 * The bar lines, every fourth one brighter.
 *
 * Two judgements in here, both about not lying:
 *
 * **Nothing is drawn when the lines would be closer than four pixels.** Below
 * that a grid stops being a grid and becomes a wash the eye reads as part of the
 * signal — a quiet passage would look busier than it is. Nothing beats a tint
 * that misinforms.
 *
 * **Every fourth line is brighter**, because four bars is the phrase almost
 * everything in the archive is built on, and counting single bars across a
 * screen is exactly the work the grid is supposed to remove.
 */
private fun DrawScope.gridLines(
    frames: Long,
    samplesPerBar: Double?,
    left: Float,
    zoom: Float,
) {
    if (samplesPerBar == null || samplesPerBar <= 0.0 || frames <= 0L) return

    val step = size.width * zoom * (samplesPerBar / frames).toFloat()
    if (!step.isFinite() || step < 4f) return

    val bars = (frames / samplesPerBar).toInt()
    if (bars < 1) return

    val shift = left * zoom * size.width
    for (bar in 1..bars) {
        val x = step * bar - shift
        if (x >= size.width) break
        if (x < 0f) continue
        val phrase = bar % 4 == 0
        drawRect(
            color = if (phrase) Palette.gridStrong else Palette.grid,
            topLeft = Offset(x, 0f),
            size = Size(1f, size.height),
        )
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
    /** Bar lines. Under the waveform, so barely there is right. */
    val grid = Color(0xFF23232C)
    /** Every fourth bar — the phrase line. */
    val gridStrong = Color(0xFF3A3A47)
    val marker = Color(0xFFF2F2F2)
    /** A marker under a finger. Deliberately not the play head's colour. */
    val grabbed = Color(0xFF7DD3FC)
    val playHead = Color(0xFF4ADE80)
    val text = Color(0xFFE8E8EC)
    val dim = Color(0xFF8A8A96)
    /** The unfilled part of a slider track. */
    val trackIdle = Color(0xFF3A3A47)
    /** The border of an open group. */
    val outline = Color(0xFF3A3A47)
    /** The border of a folded one — present, but not asking for attention. */
    val outlineIdle = Color(0xFF26262F)
    val warn = Color(0xFFFFB020)
    val bad = Color(0xFFFF4D4D)
}
