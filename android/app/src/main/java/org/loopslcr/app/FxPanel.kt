package org.loopslcr.app

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * The insert: a filter and an overdrive, and a switch for which hears the other.
 *
 * # Why it is on the mixer page
 *
 * An insert is a place on a desk, not a room of its own. It sits between the
 * channel faders and the master, and the master meter above it is the thing that
 * tells you what turning it up did — putting it on its own tab would separate a
 * control from its meter, which is how you end up mixing by eye at a picture of
 * a different signal.
 *
 * # Why the order is a control and not a decision
 *
 * A lowpass in front of the drive takes the highs away before anything can
 * distort them: the sound stays as dark as the filter is, and the drive only
 * thickens what got through. A lowpass behind the drive takes away the highs the
 * drive itself made: everything distorts and the filter then decides how much of
 * the result reaches you — which is the sweep with something to sweep through.
 *
 * Both are things people do on purpose, so neither is wired in.
 *
 * # Why the delay and the room come last
 *
 * The filter and the drive are what the sound *is*; the delay and the room are
 * where it *is*. Putting the space first would have the drive flattening the
 * tail as well as the source, which is the sound of a broken send rather than
 * anything anyone reaches for.
 *
 * # Why the echo is given in note values
 *
 * Because that is what it is for. The division is per *bar*, so an eighth means
 * the same length of time in a four-bar loop and a thirty-two-bar one, and the
 * preview turns it into samples against the speed the loop is actually playing
 * at — pitch it up and the echo shortens with the bar instead of walking out of
 * the grid. FREE is there for the times when the point is that it does not line
 * up.
 *
 * # Why nothing here is exported
 *
 * The same reason the varispeed and the motion are not: the cut is what the file
 * is, and the insert is what your hands were doing to it. A filter sweep baked
 * into a WAV is not a loop any more.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun FxPanel(fx: Fx, onFx: (Fx) -> Unit, modifier: Modifier = Modifier) {
    Column(
        modifier.fillMaxWidth().padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("FX", color = Palette.dim, fontSize = 11.sp, fontWeight = FontWeight.Bold)
            Text(
                if (fx.isWire) "bypassed" else "in the signal path",
                color = if (fx.isWire) Palette.dim else Palette.wave,
                fontSize = 10.sp,
                fontFamily = FontFamily.Monospace,
                modifier = Modifier.testTag("fxState"),
            )
        }

        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            FxMode.entries.forEach { mode ->
                Chip(
                    label = mode.label,
                    selected = fx.mode == mode,
                    modifier = Modifier.testTag("fxMode${mode.name}"),
                ) { onFx(fx.copy(mode = mode)) }
            }
        }

        // Shown whatever the mode is, rather than hidden when the filter is off.
        // A panel whose rows come and go moves everything below them, and a knob
        // that jumps out from under a finger is worse than one that is greyed.
        Knob(
            label = "CUTOFF",
            position = cutoffTravel(fx.cutoffHz),
            reading = cutoffLabel(fx.cutoffHz),
            tag = "fxCutoff",
            enabled = fx.mode != FxMode.Off,
        ) { onFx(fx.copy(cutoffHz = cutoffFrom(it))) }

        Knob(
            label = "RESO",
            position = fx.resonance,
            reading = percent(fx.resonance),
            tag = "fxReso",
            enabled = fx.mode != FxMode.Off,
        ) { onFx(fx.copy(resonance = it)) }

        Knob(
            label = "DRIVE",
            position = fx.drive,
            reading = percent(fx.drive),
            tag = "fxDrive",
        ) { onFx(fx.copy(drive = it)) }

        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                "ORDER",
                color = Palette.dim,
                fontSize = 10.sp,
                fontFamily = FontFamily.Monospace,
                modifier = Modifier.width(62.dp),
            )
            FxRoute.entries.forEach { route ->
                Chip(
                    label = route.label,
                    selected = fx.route == route,
                    modifier = Modifier.testTag("fxRoute${route.name}"),
                ) { onFx(fx.copy(route = route)) }
            }
        }

        // Its own trim rather than making the master do the job: a filter and a
        // drive both move the level, and a knob you cannot judge without also
        // moving the master is two hands for one decision.
        Knob(
            label = "OUTPUT",
            position = travel(fx.output),
            reading = decibels(fx.output),
            tag = "fxOutput",
        ) { onFx(fx.copy(output = fromTravel(it))) }

        Divider("DELAY")

        Knob(
            label = "MIX",
            position = fx.delayMix,
            reading = percent(fx.delayMix),
            tag = "fxDelayMix",
        ) { onFx(fx.copy(delayMix = it)) }

        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                "TIME",
                color = Palette.text,
                fontSize = 10.sp,
                fontFamily = FontFamily.Monospace,
                modifier = Modifier.width(62.dp),
            )
            Chip("SYNC", fx.delaySynced, Modifier.testTag("fxDelaySync")) {
                onFx(fx.copy(delaySynced = true))
            }
            Chip("FREE", !fx.delaySynced, Modifier.testTag("fxDelayFree")) {
                onFx(fx.copy(delaySynced = false))
            }
        }

        if (fx.delaySynced) {
            // Wrapped rather than scrolled: seven note values on one line would
            // be seven targets too narrow to hit with a thumb, and this is a
            // control people reach for while something is playing.
            FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                DelayDivision.entries.forEach { division ->
                    Chip(
                        label = division.label,
                        selected = fx.delayDivision == division,
                        modifier = Modifier.testTag("fxDiv${division.name}"),
                    ) { onFx(fx.copy(delayDivision = division)) }
                }
            }
        } else {
            Knob(
                label = "MS",
                position = delayTravel(fx.delayMs),
                reading = millis(fx.delayMs),
                tag = "fxDelayMs",
            ) { onFx(fx.copy(delayMs = delayMsFrom(it))) }
        }

        Knob(
            label = "FEEDBACK",
            position = fx.delayFeedback,
            reading = percent(fx.delayFeedback),
            tag = "fxDelayFeedback",
        ) { onFx(fx.copy(delayFeedback = it)) }

        Knob(
            label = "DAMP",
            position = fx.delayDamping,
            reading = percent(fx.delayDamping),
            tag = "fxDelayDamp",
        ) { onFx(fx.copy(delayDamping = it)) }

        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Spacer(Modifier.width(56.dp))
            Chip("PING-PONG", fx.pingPong, Modifier.testTag("fxPingPong")) {
                onFx(fx.copy(pingPong = !fx.pingPong))
            }
            // Freeze holds the line and stops listening. Feedback cannot be
            // pushed to unity, so this is the only honest way to ask for
            // forever — and it says so on the switch rather than hiding at the
            // top of a knob.
            Chip("FREEZE", fx.freeze, Modifier.testTag("fxFreeze")) {
                onFx(fx.copy(freeze = !fx.freeze))
            }
        }

        Divider("ROOM")

        Knob(
            label = "MIX",
            position = fx.reverbMix,
            reading = percent(fx.reverbMix),
            tag = "fxReverbMix",
        ) { onFx(fx.copy(reverbMix = it)) }

        Knob(
            label = "SIZE",
            position = fx.reverbSize,
            reading = percent(fx.reverbSize),
            tag = "fxReverbSize",
            enabled = fx.reverbMix > 0f,
        ) { onFx(fx.copy(reverbSize = it)) }

        Knob(
            label = "DAMP",
            position = fx.reverbDamping,
            reading = percent(fx.reverbDamping),
            tag = "fxReverbDamp",
            enabled = fx.reverbMix > 0f,
        ) { onFx(fx.copy(reverbDamping = it)) }

        // A room that answers instantly buries the transient it is answering.
        // A few tens of milliseconds leaves the drum hit in the clear and puts
        // the room behind it.
        Knob(
            label = "PRE-DLY",
            position = fx.reverbPredelayMs / FX_MAX_PREDELAY_MS,
            reading = millis(fx.reverbPredelayMs),
            tag = "fxReverbPre",
            enabled = fx.reverbMix > 0f,
        ) { onFx(fx.copy(reverbPredelayMs = it * FX_MAX_PREDELAY_MS)) }

        Text(
            "The insert sits after both channel faders and before the master, " +
                "so the master meter is what says whether it got too loud. " +
                "None of it is exported — the cut is what the file is.",
            color = Palette.dim,
            fontSize = 11.sp,
        )
    }
}

/** A named rule, so the three boxes read as three boxes. */
@Composable
private fun Divider(label: String) {
    Row(
        Modifier.fillMaxWidth().padding(top = 6.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, color = Palette.dim, fontSize = 10.sp, fontWeight = FontWeight.Bold)
        Box(Modifier.weight(1f).height(1.dp).background(Palette.trackIdle))
    }
}

/** One labelled slider with its number, so every knob here reads the same way. */
@Composable
private fun Knob(
    label: String,
    position: Float,
    reading: String,
    tag: String,
    enabled: Boolean = true,
    onPosition: (Float) -> Unit,
) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Text(
            label,
            color = if (enabled) Palette.text else Palette.dim,
            fontSize = 10.sp,
            fontFamily = FontFamily.Monospace,
            modifier = Modifier.width(62.dp),
        )
        ThinSlider(
            value = position.coerceIn(0f, 1f),
            onValueChange = { if (enabled) onPosition(it) },
            valueRange = 0f..1f,
            modifier = Modifier.weight(1f).testTag(tag),
        )
        Text(
            reading,
            color = if (enabled) Palette.text else Palette.dim,
            fontSize = 11.sp,
            fontFamily = FontFamily.Monospace,
            textAlign = TextAlign.End,
            modifier = Modifier.width(76.dp),
        )
    }
}
