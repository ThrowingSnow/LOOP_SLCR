package org.loopslcr.app

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * The second loop.
 *
 * # Why this is not a second cutter
 *
 * It has no varispeed and no export, because it has no length of its own. Its
 * whole job is to fit the first loop: cut to the same bar count and pulled to
 * the same tempo, so that one play head can serve both and there is nothing to
 * keep in step. Giving it a speed control would be offering to break the one
 * property that makes the pair work.
 *
 * What it does get is the two decisions that are genuinely its own — which part
 * of the file to use, and how it is read.
 *
 * # Why the fit is stated rather than assumed
 *
 * Pulling this loop to the other's tempo is exact only because both tempi are
 * known exactly. When one of them is not — a file with no tempo anywhere, a bar
 * count that does not divide — the result is a loop of the wrong length, and the
 * pair refuses it rather than stretching it silently. That refusal arrives here,
 * with both numbers in it.
 */
@Composable
fun SecondScreen(
    first: Loaded?,
    firstPlan: Plan?,
    loaded: Loaded?,
    settings: Settings,
    plan: Plan?,
    problem: String?,
    onOpen: () -> Unit,
    onDrop: () -> Unit,
    onChange: ((Settings) -> Settings) -> Unit,
) {
    Column(
        Modifier
            .fillMaxSize()
            .background(Palette.background)
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 12.dp, vertical = 10.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        if (first == null) {
            Text(
                "Open a loop in CUTTER 1 first. The second loop is cut to fit " +
                    "the first one, so there has to be a first one.",
                color = Palette.dim,
                fontSize = 13.sp,
            )
            return@Column
        }

        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.width(220.dp)) {
                Text(
                    "SECOND LOOP",
                    color = Palette.dim,
                    fontSize = 11.sp,
                    fontWeight = FontWeight.Bold,
                )
                Text(
                    loaded?.name ?: "no file",
                    color = Palette.dim,
                    fontSize = 12.sp,
                    fontFamily = FontFamily.Monospace,
                )
            }
            if (loaded == null) {
                OutlinedButton(onClick = onOpen, modifier = Modifier.testTag("openSecond")) {
                    Text("Open")
                }
            } else {
                OutlinedButton(onClick = onDrop) { Text("Remove") }
            }
        }

        if (loaded == null) {
            Text(
                "A second drum loop, played from the same play head as the " +
                    "first: bar three of one followed by bar four of the other, " +
                    "in time, because neither is ever restarted.",
                color = Palette.dim,
                fontSize = 12.sp,
            )
            return@Column
        }

        Panel {
            Waveform(
                peaks = loaded.peaks,
                channels = loaded.analysis.channels,
                frames = loaded.analysis.frames,
                region = plan?.let { it.regionStart..it.regionEnd },
                samplesPerBar = plan?.samplesPerBar,
                folded = true,
                modifier = Modifier
                    .fillMaxWidth()
                    .height(84.dp)
                    .testTag("secondWave"),
            )
        }

        // The fit, in numbers. This is the one thing worth saying loudest: it is
        // where the pair succeeds or fails, and the numbers are what a user can
        // act on — the bar count is theirs to change.
        Fit(first, firstPlan, loaded, plan)

        if (problem != null) {
            Text(problem, color = Palette.bad, fontSize = 12.sp)
        }

        Section("Loop", initiallyOpen = true, summary = plan?.let { "${it.bars} bars from ${it.skipBars}" }) {
            Text(
                "The bar count comes from the first loop — both have to be the " +
                    "same length. What is yours to choose is where in this file " +
                    "those bars start.",
                color = Palette.dim,
                fontSize = 11.sp,
            )
            SkipRow(settings, onChange)
            WorkflowRow(settings, plan, onChange)
        }

        Section("Source", summary = plan?.let { "${trim(it.tempo)} BPM · ${settings.sig}" }) {
            SourceControls(loaded.analysis, settings, onChange)
        }

        Section("Tape", summary = if (settings.tape) "on" else "off") {
            TapeControls(settings, onChange)
        }

        Spacer(Modifier.height(24.dp))
    }
}

/** Whether the two loops are the same length, and by how much they are not. */
@Composable
private fun Fit(first: Loaded, firstPlan: Plan?, second: Loaded, secondPlan: Plan?) {
    Panel {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Fact("first", "${firstPlan?.bars ?: "—"} bars · ${firstPlan?.let { trim(it.tempo) } ?: "—"} BPM")
            Fact("second", "${secondPlan?.bars ?: "—"} bars · ${secondPlan?.let { trim(it.tempo) } ?: "—"} BPM")

            val mine = firstPlan?.outputFrames
            val theirs = secondPlan?.outputFrames
            Fact("frames", "${theirs ?: "—"} against ${mine ?: "—"}")

            when {
                first.analysis.sampleRate != second.analysis.sampleRate -> Text(
                    "different sample rates — ${second.analysis.sampleRate} Hz " +
                        "against ${first.analysis.sampleRate} Hz. One play head " +
                        "cannot serve two rates; convert one of them first.",
                    color = Palette.bad,
                    fontSize = 11.sp,
                )
                mine != null && theirs != null && mine != theirs -> Text(
                    "${theirs - mine} frames out. Usually a tempo one of the " +
                        "files does not declare — set it under SOURCE and the " +
                        "arithmetic becomes exact.",
                    color = Palette.warn,
                    fontSize = 11.sp,
                )
                mine != null && theirs == mine -> Text(
                    "the same length, exactly",
                    color = Palette.dim,
                    fontSize = 11.sp,
                )
            }
        }
    }
}
