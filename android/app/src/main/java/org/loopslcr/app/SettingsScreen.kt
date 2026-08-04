package org.loopslcr.app

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * What the app is, and what it will let you change about itself.
 *
 * Deliberately mostly empty. The tab exists because appearance, controls and
 * haptics are wanted, and giving them a home now settles where they go before
 * there are three of them scattered through the cutter. What is here today is
 * the one thing that has already cost real time to work out by hand: which build
 * is running.
 *
 * Nothing is invented for it. A settings screen listing switches that do not do
 * anything yet would be worse than a short one — it would describe an app that
 * does not exist. The DISPLAY section is here because both of its choices do
 * something today.
 */
@Composable
fun SettingsScreen(
    engineVersion: String,
    build: String,
    display: Display = Display(),
    onDisplay: (Display) -> Unit = {},
) {
    Column(
        Modifier
            .fillMaxSize()
            .background(Palette.background)
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text("LOOP_SLCR", color = Palette.text, fontSize = 18.sp)

        // The version code as well as the name. It counts commits, so it rises
        // every build — which is what makes "did the install actually take?" a
        // question with an answer rather than a guess.
        Text(
            "build    $build (${BuildConfig.VERSION_CODE})\nengine   $engineVersion",
            color = Palette.dim,
            fontSize = 12.sp,
            fontFamily = FontFamily.Monospace,
        )

        Spacer(Modifier.height(8.dp))
        Text("DISPLAY", color = Palette.dim, fontSize = 11.sp)

        Text("Mixer", color = Palette.text, fontSize = 13.sp)
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            Choice("desk", display.mixer == MixerView.Desk, "mixerDesk") {
                onDisplay(display.copy(mixer = MixerView.Desk))
            }
            Choice("rows", display.mixer == MixerView.Rows, "mixerRows") {
                onDisplay(display.copy(mixer = MixerView.Rows))
            }
        }
        Text(
            "The desk stands three faders side by side, which is how two levels " +
                "get compared at a glance. Rows give each channel the whole " +
                "width instead. Same numbers either way.",
            color = Palette.dim,
            fontSize = 11.sp,
        )

        Text("Orientation", color = Palette.text, fontSize = 13.sp)
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            Choice("auto", display.screen == Screen.Auto, "screenAuto") {
                onDisplay(display.copy(screen = Screen.Auto))
            }
            Choice("portrait", display.screen == Screen.Portrait, "screenPortrait") {
                onDisplay(display.copy(screen = Screen.Portrait))
            }
            Choice("landscape", display.screen == Screen.Landscape, "screenLandscape") {
                onDisplay(display.copy(screen = Screen.Landscape))
            }
        }
        Text(
            "Sideways, the waveform takes the left half and the panels stand " +
                "beside it rather than under it. Auto lets the phone decide, " +
                "which it is better at than a setting is — the other two are " +
                "for a phone lying on a desk.",
            color = Palette.dim,
            fontSize = 11.sp,
        )

        Spacer(Modifier.height(8.dp))
        Text("COMING HERE", color = Palette.dim, fontSize = 11.sp)
        Text(
            "Controls and haptics. They are named rather than shown, because a " +
                "switch that does nothing is a lie with a nice finish.",
            color = Palette.dim,
            fontSize = 12.sp,
        )

        Spacer(Modifier.height(8.dp))
        Text("WHAT THIS TOOL PROMISES", color = Palette.dim, fontSize = 11.sp)
        Text(
            "Cut points come from exact rational arithmetic, never from floating " +
                "point. The same file and the same settings always produce the " +
                "same bytes. Nothing is read but the file you pick, and nothing " +
                "leaves this device.",
            color = Palette.dim,
            fontSize = 12.sp,
        )
    }
}

/** One of a small set, chosen by tapping it. */
@Composable
private fun Choice(label: String, selected: Boolean, tag: String, onClick: () -> Unit) {
    FilterChip(
        selected = selected,
        onClick = onClick,
        label = { Text(label, fontSize = 12.sp) },
        modifier = Modifier.testTag(tag),
    )
}
