package org.loopslcr.app

import androidx.compose.foundation.background
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
import androidx.compose.foundation.shape.RoundedCornerShape
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
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlin.math.log10
import kotlin.math.pow

/**
 * The two loops, and how loud each of them is.
 *
 * # Why the meters read zero half the time
 *
 * They are per *loop*, not per output, and with a swap running only one loop is
 * sounding at a time. A meter showing the sum could not answer the question this
 * page exists for — "which of these two is too loud" — so it shows each one's
 * own contribution and lets the silent one read silent. During a swap's
 * crossfade both move, briefly, which is exactly what is happening.
 *
 * # Why the trim is not in dB under the finger
 *
 * The slider is linear in *loudness*, not in amplitude: a slider halfway along
 * sounds about half as loud, which is what a hand expects. The number beside it
 * is in dB, which is what an ear talks in. Doing it the other way round gives a
 * control that does almost nothing over most of its travel.
 */
@Composable
fun MixerScreen(
    first: Loaded?,
    second: Loaded?,
    gains: Pair<Float, Float>,
    levels: () -> Pair<Float, Float>,
    playing: Boolean,
    onGains: (Float, Float) -> Unit,
) {
    // Polled like the play head, and for the same reason: the audio thread
    // publishes to an atomic and must not be made to notify anyone.
    var shown by remember { mutableFloatStateOf(0f) }
    var shownSecond by remember { mutableFloatStateOf(0f) }
    LaunchedEffect(playing) {
        if (!playing) {
            shown = 0f
            shownSecond = 0f
            return@LaunchedEffect
        }
        while (isActive) {
            val (a, b) = levels()
            // Instant rise, slow fall. A meter that fell as fast as the audio
            // does would flicker at every drum hit and read as noise; holding
            // the peak briefly is what makes it legible.
            shown = if (a > shown) a else shown * 0.82f
            shownSecond = if (b > shownSecond) b else shownSecond * 0.82f
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

        Channel(
            index = 1,
            name = first.name,
            gain = gains.first,
            level = shown,
            onGain = { onGains(it, gains.second) },
        )

        if (second == null) {
            Text(
                "No second loop. Open one in CUTTER 2 and it appears here.",
                color = Palette.dim,
                fontSize = 12.sp,
            )
        } else {
            Channel(
                index = 2,
                name = second.name,
                gain = gains.second,
                level = shownSecond,
                onGain = { onGains(gains.first, it) },
            )
        }

        if (!playing) {
            Text(
                "The meters move while the loop plays.",
                color = Palette.dim,
                fontSize = 11.sp,
            )
        }

        Spacer(Modifier.height(8.dp))
        Text("COMING HERE", color = Palette.dim, fontSize = 11.sp)
        Text(
            "Delay and reverb, on their own page. Named rather than shown, for " +
                "the same reason the settings tab is mostly empty: a control " +
                "that does nothing is worse than one that is not there yet.",
            color = Palette.dim,
            fontSize = 12.sp,
        )
    }
}

@Composable
private fun Channel(
    index: Int,
    name: String,
    gain: Float,
    level: Float,
    onGain: (Float) -> Unit,
) {
    Panel {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text(
                    "$index",
                    color = Palette.text,
                    fontSize = 14.sp,
                    fontWeight = FontWeight.Bold,
                )
                Spacer(Modifier.width(8.dp))
                Text(
                    name,
                    color = Palette.dim,
                    fontSize = 11.sp,
                    fontFamily = FontFamily.Monospace,
                    maxLines = 1,
                    modifier = Modifier.weight(1f),
                )
                Text(
                    decibels(gain),
                    color = Palette.text,
                    fontSize = 12.sp,
                    fontFamily = FontFamily.Monospace,
                )
            }

            Meter(level, Modifier.testTag("meter$index"))

            ThinSlider(
                value = loudness(gain),
                onValueChange = { onGain(fromLoudness(it)) },
                valueRange = 0f..1f,
                modifier = Modifier.fillMaxWidth().testTag("gain$index"),
            )
        }
    }
}

/**
 * A peak bar, marked where clipping starts.
 *
 * Drawn on a decibel scale rather than a linear one: linear, everything quiet
 * enough to be worth adjusting sits in the leftmost tenth of the bar and the
 * meter is decoration.
 */
@Composable
private fun Meter(level: Float, modifier: Modifier = Modifier) {
    val filled = meterScale(level)
    Box(
        modifier
            .fillMaxWidth()
            .height(10.dp)
            .background(Palette.trackIdle, RoundedCornerShape(2.dp)),
    ) {
        Box(
            Modifier
                .fillMaxWidth(filled)
                .fillMaxHeight()
                .background(
                    if (level >= 1f) Palette.bad else Palette.wave,
                    RoundedCornerShape(2.dp),
                ),
        )
    }
}

/** Where a level sits on a bar that runs from −48 dB to full scale. */
internal fun meterScale(level: Float): Float {
    if (level <= 0f) return 0f
    val db = 20f * log10(level)
    return ((db + 48f) / 48f).coerceIn(0f, 1f)
}

/** A linear gain as the position of a slider that feels even to a hand. */
internal fun loudness(gain: Float): Float =
    if (gain <= 0f) 0f else (gain.toDouble().pow(1.0 / 3.0)).toFloat().coerceIn(0f, 1f)

internal fun fromLoudness(position: Float): Float =
    (position.toDouble().pow(3.0)).toFloat().coerceIn(0f, 1f)

internal fun decibels(gain: Float): String = when {
    gain <= 0f -> "  −∞ dB"
    else -> "%+5.1f dB".format(20f * log10(gain))
}
