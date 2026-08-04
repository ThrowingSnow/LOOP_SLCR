package org.loopslcr.app

import androidx.compose.foundation.layout.Arrangement
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
 * # Why nothing here is exported
 *
 * The same reason the varispeed and the motion are not: the cut is what the file
 * is, and the insert is what your hands were doing to it. A filter sweep baked
 * into a WAV is not a loop any more.
 */
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

        Text(
            "The insert sits after both channel faders and before the master, " +
                "so the master meter is what says whether it got too loud.",
            color = Palette.dim,
            fontSize = 11.sp,
        )
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
