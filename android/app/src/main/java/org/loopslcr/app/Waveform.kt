package org.loopslcr.app

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.input.pointer.pointerInput
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
 */
@Composable
fun Waveform(
    peaks: FloatArray,
    channels: Int,
    frames: Long,
    region: LongRange?,
    /** Where the preview is, as a fraction of the whole file, or null when stopped. */
    playHead: Float? = null,
    /** Called while a marker is dragged, with its new position as a fraction. */
    onDrag: ((Marker, Float) -> Unit)? = null,
    modifier: Modifier = Modifier,
) {
    // Which marker the finger grabbed, decided once when the drag starts.
    // Re-deciding per movement would let a fast drag hand the gesture to the
    // other marker halfway through and swap the ends of the loop.
    var grabbed by remember { mutableStateOf(Marker.Start) }

    val gestures = if (onDrag == null || region == null || frames <= 0) {
        Modifier
    } else {
        Modifier.pointerInput(frames, region.first, region.last) {
            detectDragGestures(
                onDragStart = { down ->
                    val start = size.width * (region.first.toFloat() / frames)
                    val end = size.width * (region.last.toFloat() / frames)
                    grabbed = if (abs(down.x - start) <= abs(down.x - end)) {
                        Marker.Start
                    } else {
                        Marker.End
                    }
                },
                onDrag = { change, _ ->
                    change.consume()
                    onDrag(grabbed, (change.position.x / size.width).coerceIn(0f, 1f))
                },
            )
        }
    }

    Canvas(modifier.then(gestures)) {
        val buckets = if (channels > 0) peaks.size / (channels * 2) else 0
        if (buckets == 0) return@Canvas

        drawRect(Palette.waveBackground)

        // Everything outside the cut is dimmed rather than hidden: the user is
        // choosing a region *of a file*, and a file whose discarded parts have
        // vanished gives them nothing to judge the choice against.
        if (region != null && frames > 0) {
            val left = size.width * (region.first.toFloat() / frames)
            val right = size.width * (region.last.toFloat() / frames)
            drawRect(Palette.outside, Offset(0f, 0f), Size(left, size.height))
            drawRect(Palette.outside, Offset(right, 0f), Size(size.width - right, size.height))
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

        if (region != null && frames > 0) {
            marker(size.width * (region.first.toFloat() / frames))
            marker(size.width * (region.last.toFloat() / frames))
        }

        if (playHead != null) {
            val x = size.width * playHead.coerceIn(0f, 1f)
            drawRect(Palette.playHead, Offset(x - 1f, 0f), Size(3f, size.height))
        }
    }
}

private fun DrawScope.marker(x: Float) {
    drawRect(Palette.marker, Offset(x - 1f, 0f), Size(2f, size.height))
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
    val playHead = Color(0xFF4ADE80)
    val text = Color(0xFFE8E8EC)
    val dim = Color(0xFF8A8A96)
    val warn = Color(0xFFFFB020)
    val bad = Color(0xFFFF4D4D)
}
