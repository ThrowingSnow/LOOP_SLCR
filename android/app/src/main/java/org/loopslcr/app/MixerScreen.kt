package org.loopslcr.app

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlin.math.log10
import kotlin.math.pow

/**
 * The console: two channels and a master, standing side by side.
 *
 * # Why it looks like a desk
 *
 * Because it is one, and a desk is read across rather than down. Three strips
 * next to each other let one glance compare two levels; three rows of
 * horizontal sliders make that a scroll and a memory test. The faders are
 * vertical for the same reason every mixer's are — the eye compares heights.
 *
 * # Why there is a rows view as well
 *
 * A desk needs width, and a phone held upright has little of it — three strips
 * on a narrow screen are three narrow strips. Rows give each channel the whole
 * width and the file name room to be read, at the cost of the comparison the
 * desk is for. Which of those matters is not something this file can know, so
 * it is a setting rather than a guess: SETTINGS → DISPLAY.
 *
 * The two views share every number. Same meter scale, same fader travel, same
 * detent at unity — only the direction changes, because a control that behaves
 * differently depending on how it is drawn is two controls wearing one name.
 *
 * # Why the channel meters ignore the master
 *
 * They are post-*their* fader and pre-master, which is the console arrangement:
 * the channels keep saying which loop is loud while the master says whether
 * what leaves is too loud. A master pull that moved all three meters would tell
 * you the same thing three times and lose the balance.
 *
 * # Why the meters read zero half the time
 *
 * They are per *loop*, and with a swap running only one loop is sounding. A
 * meter showing the sum could not answer the question this page exists for —
 * "which of these two is too loud" — so the silent one reads silent. During a
 * swap's crossfade both move, briefly, which is exactly what is happening.
 *
 * # Why the travel is not in dB
 *
 * The fader is linear in *loudness*: halfway up sounds about half as loud,
 * which is what a hand expects. The number under it is in dB, which is what an
 * ear talks in. Doing it the other way round gives a control that does almost
 * nothing over most of its travel.
 */
@Composable
fun MixerScreen(
    first: Loaded?,
    second: Loaded?,
    gains: Pair<Float, Float>,
    masterGain: Float,
    levels: () -> Triple<Float, Float, Float>,
    playing: Boolean,
    onGains: (Float, Float) -> Unit,
    onMasterGain: (Float) -> Unit,
    fx: Fx = Fx(),
    onFx: (Fx) -> Unit = {},
    view: MixerView = MixerView.Desk,
) {
    // Polled like the play head, and for the same reason: the audio thread
    // publishes to an atomic and must not be made to notify anyone.
    var one by remember { mutableFloatStateOf(0f) }
    var two by remember { mutableFloatStateOf(0f) }
    var out by remember { mutableFloatStateOf(0f) }
    LaunchedEffect(playing) {
        if (!playing) {
            one = 0f
            two = 0f
            out = 0f
            return@LaunchedEffect
        }
        while (isActive) {
            val (a, b, m) = levels()
            // Instant rise, slow fall. A meter that fell as fast as the audio
            // does would flicker at every drum hit and read as noise; holding
            // the peak briefly is what makes it legible.
            one = if (a > one) a else one * 0.82f
            two = if (b > two) b else two * 0.82f
            out = if (m > out) m else out * 0.82f
            delay(33)
        }
    }

    Column(
        Modifier
            .fillMaxSize()
            .background(Palette.background)
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 12.dp, vertical = 10.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Text("MIXER", color = Palette.dim, fontSize = 11.sp, fontWeight = FontWeight.Bold)

        if (first == null) {
            Text(
                "Nothing loaded. Open a loop in CUTTER 1.",
                color = Palette.dim,
                fontSize = 13.sp,
            )
            return@Column
        }

        val channelOne = @Composable { desk: Boolean ->
            Strip(
                tag = "1",
                name = first.name,
                gain = gains.first,
                level = one,
                master = false,
                desk = desk,
                onGain = { onGains(it, gains.second) },
                modifier = if (desk) Modifier.weight(1f) else Modifier.fillMaxWidth(),
            )
        }
        val channelTwo = @Composable { desk: Boolean ->
            Strip(
                tag = "2",
                name = second?.name ?: "no second loop",
                gain = gains.second,
                level = two,
                master = false,
                desk = desk,
                // A strip with nothing under it is shown dead rather than
                // hidden, so the desk keeps its shape while you load one.
                enabled = second != null,
                onGain = { onGains(gains.first, it) },
                modifier = if (desk) Modifier.weight(1f) else Modifier.fillMaxWidth(),
            )
        }
        val master = @Composable { desk: Boolean ->
            Strip(
                tag = "MST",
                name = "what leaves",
                gain = masterGain,
                level = out,
                master = true,
                desk = desk,
                onGain = onMasterGain,
                modifier = if (desk) Modifier.weight(1f) else Modifier.fillMaxWidth(),
            )
        }

        if (view == MixerView.Desk) {
            Panel {
                Row(
                    Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 10.dp, vertical = 12.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    channelOne(true)
                    channelTwo(true)
                    master(true)
                }
            }
        } else {
            Panel { Box(Modifier.padding(12.dp)) { channelOne(false) } }
            Panel { Box(Modifier.padding(12.dp)) { channelTwo(false) } }
            Panel { Box(Modifier.padding(12.dp)) { master(false) } }
        }

        if (!playing) {
            Text(
                "The meters move while the loop plays.",
                color = Palette.dim,
                fontSize = 11.sp,
            )
        }

        Text(
            "Channels are metered before the master, so pulling the master " +
                "down does not make the balance look different — only what " +
                "leaves gets quieter.",
            color = Palette.dim,
            fontSize = 11.sp,
        )

        Panel { FxPanel(fx = fx, onFx = onFx) }

        Spacer(Modifier.height(8.dp))
        Text("COMING HERE", color = Palette.dim, fontSize = 11.sp)
        Text(
            "Delay and reverb, in the same insert. Named rather than shown, for " +
                "the same reason the settings tab is mostly empty: a control " +
                "that does nothing is worse than one that is not there yet.",
            color = Palette.dim,
            fontSize = 12.sp,
        )
    }
}

/** One channel: a name, a meter, a fader and its number, either way up. */
@Composable
private fun Strip(
    tag: String,
    name: String,
    gain: Float,
    level: Float,
    master: Boolean,
    desk: Boolean,
    onGain: (Float) -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    if (!desk) {
        Row(modifier, verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.width(96.dp)) {
                Text(
                    tag,
                    color = if (master) Palette.wave else Palette.text,
                    fontSize = 13.sp,
                    fontWeight = FontWeight.Bold,
                    fontFamily = FontFamily.Monospace,
                )
                Text(
                    name,
                    color = Palette.dim,
                    fontSize = 9.sp,
                    fontFamily = FontFamily.Monospace,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Meter(
                    level,
                    vertical = false,
                    modifier = Modifier.fillMaxWidth().height(8.dp).testTag("meter$tag"),
                )
                // The same travel as the desk's fader, lying down: the value a
                // finger lands on must not depend on which view is showing.
                ThinSlider(
                    value = travel(gain),
                    onValueChange = { if (enabled) onGain(fromTravel(it)) },
                    valueRange = 0f..1f,
                    modifier = Modifier.fillMaxWidth().testTag("fader$tag"),
                )
            }
            Text(
                if (enabled) decibels(gain) else "—",
                color = if (enabled) Palette.text else Palette.dim,
                fontSize = 11.sp,
                fontFamily = FontFamily.Monospace,
                modifier = Modifier.width(64.dp),
                textAlign = TextAlign.End,
            )
        }
        return
    }

    Column(
        modifier,
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Text(
            tag,
            color = if (master) Palette.wave else Palette.text,
            fontSize = 13.sp,
            fontWeight = FontWeight.Bold,
            fontFamily = FontFamily.Monospace,
        )
        Text(
            name,
            color = Palette.dim,
            fontSize = 9.sp,
            fontFamily = FontFamily.Monospace,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            textAlign = TextAlign.Center,
            modifier = Modifier.fillMaxWidth(),
        )

        Row(
            Modifier.height(190.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Meter(
                level,
                vertical = true,
                modifier = Modifier.width(10.dp).fillMaxHeight().testTag("meter$tag"),
            )
            Fader(
                gain = gain,
                enabled = enabled,
                onGain = onGain,
                modifier = Modifier.width(44.dp).testTag("fader$tag"),
            )
        }

        Text(
            if (enabled) decibels(gain) else "—",
            color = if (enabled) Palette.text else Palette.dim,
            fontSize = 11.sp,
            fontFamily = FontFamily.Monospace,
        )
    }
}

/**
 * A peak bar, on a decibel scale.
 *
 * Linear, everything quiet enough to be worth adjusting would sit in the first
 * tenth and the meter would be decoration.
 */
@Composable
private fun Meter(level: Float, vertical: Boolean, modifier: Modifier = Modifier) {
    Canvas(modifier.fillMaxSize()) {
        drawRect(Palette.trackIdle, size = size)
        val colour = if (level >= 1f) Palette.bad else Palette.wave
        val filled = meterScale(level)
        if (filled > 0f) {
            if (vertical) {
                val high = filled * size.height
                drawRect(colour, Offset(0f, size.height - high), Size(size.width, high))
            } else {
                drawRect(colour, Offset(0f, 0f), Size(filled * size.width, size.height))
            }
        }
        // Where clipping starts, so a bar near the end can be read as near it
        // rather than merely long.
        val unity = meterScale(1f)
        if (vertical) {
            val y = size.height - unity * size.height
            drawRect(Palette.dim, Offset(0f, y), Size(size.width, 1f))
        } else {
            drawRect(Palette.dim, Offset(unity * size.width - 1f, 0f), Size(1f, size.height))
        }
    }
}

/**
 * A fader: drag it, or tap where you want it.
 *
 * Hand-drawn rather than a rotated slider. A `Slider` turned on its side keeps
 * measuring and reporting itself in its old orientation, which makes it a
 * control that lies to the layout and to the tests — and this one has to be
 * exactly as tall as the meter beside it to be worth reading against it.
 */
@Composable
private fun Fader(
    gain: Float,
    enabled: Boolean,
    onGain: (Float) -> Unit,
    modifier: Modifier = Modifier,
) {
    var height by remember { mutableFloatStateOf(1f) }
    val take = { y: Float ->
        if (enabled) onGain(fromTravel(1f - (y / height).coerceIn(0f, 1f)))
    }

    Canvas(
        modifier
            .fillMaxSize()
            .pointerInput(enabled) {
                detectTapGestures { take(it.y) }
            }
            .pointerInput(enabled) {
                detectVerticalDragGestures { change, _ -> take(change.position.y) }
            },
    ) {
        height = size.height
        val travel = travel(gain)
        val cap = 14.dp.toPx()
        // The cap's own height is taken out of the travel, so the top of the
        // stroke is reachable and the fader is not half a cap short at each end.
        val span = size.height - cap
        val top = span * (1f - travel)
        val middle = size.width / 2f

        drawRect(
            Palette.trackIdle,
            topLeft = Offset(middle - 2.dp.toPx(), cap / 2f),
            size = Size(4.dp.toPx(), span),
        )
        // Unity, marked. A fader that can go above it has to say where it is,
        // or "loud enough" becomes a thing you find by ear every time.
        val unity = cap / 2f + span * (1f - travel(1f))
        drawRect(
            Palette.dim,
            topLeft = Offset(middle - 10.dp.toPx(), unity),
            size = Size(20.dp.toPx(), 1f),
        )
        drawRect(
            if (enabled) Palette.wave else Palette.dim.copy(alpha = 0.4f),
            topLeft = Offset(middle - 16.dp.toPx(), top),
            size = Size(32.dp.toPx(), cap),
        )
        drawRect(
            Color.Black.copy(alpha = 0.55f),
            topLeft = Offset(middle - 16.dp.toPx(), top + cap / 2f - 0.5.dp.toPx()),
            size = Size(32.dp.toPx(), 1.dp.toPx()),
        )
    }
}

/** Where a level sits on a bar that runs from −48 dB to full scale. */
internal fun meterScale(level: Float): Float {
    if (level <= 0f) return 0f
    val db = 20f * log10(level)
    return ((db + 48f) / 48f).coerceIn(0f, 1f)
}

/**
 * Fader travel as a gain, and back.
 *
 * Cube law, so the travel is even to a hand rather than even in amplitude —
 * and scaled so that the top is +6 dB and unity sits a little below it, where a
 * desk puts it. A fader that could only ever cut would make "this loop is too
 * quiet" a problem with no control on the page.
 */
internal fun fromTravel(position: Float): Float {
    val raw = (MAX_GAIN * position.toDouble().pow(3.0)).toFloat()
    // A detent at unity, for the same reason the varispeed has one at zero: a
    // fader that cannot land on exactly 0 dB leaves a level nobody chose.
    return if (kotlin.math.abs(position - UNITY_TRAVEL) < 0.02f) 1f else raw.coerceIn(0f, MAX_GAIN)
}

internal fun travel(gain: Float): Float =
    if (gain <= 0f) 0f else ((gain / MAX_GAIN).toDouble().pow(1.0 / 3.0)).toFloat().coerceIn(0f, 1f)

/** The top of every fader: +6 dB, in linear gain. */
internal const val MAX_GAIN = 2f

/** Where unity sits on that travel — `(1/2)^(1/3)`, about four fifths up. */
internal val UNITY_TRAVEL = travel(1f)

internal fun decibels(gain: Float): String = when {
    gain <= 0f -> " −∞ dB"
    else -> "%+.1f dB".format(20f * log10(gain))
}
