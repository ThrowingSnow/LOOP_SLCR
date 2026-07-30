package org.loopslcr.app

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilterChip
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlin.math.abs
import kotlin.math.roundToInt

/** Where the detent at unity ends. Inside this, the varispeed is exactly 1.0. */
private const val DETENT_SEMITONES = 0.25

@Composable
fun CutterScreen(
    loaded: Loaded?,
    settings: Settings,
    plan: Plan?,
    busy: Busy,
    problem: String?,
    onOpen: () -> Unit,
    onExport: () -> Unit,
    onChange: ((Settings) -> Settings) -> Unit,
    onDismissProblem: () -> Unit,
) {
    Column(
        Modifier
            .fillMaxSize()
            .background(Palette.background)
            // Before the scroll, so the inset is a margin the content sits
            // inside rather than something that scrolls away and lets the title
            // slide under the clock.
            .windowInsetsPadding(WindowInsets.safeDrawing)
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Header(loaded, onOpen)

        if (problem != null) Problem(problem, onDismissProblem)

        if (loaded == null) {
            Spacer(Modifier.height(24.dp))
            Text(
                "Pick a WAVE file to cut. Nothing is read until you do, and " +
                    "nothing outside it is ever read.",
                color = Palette.dim,
            )
            return@Column
        }

        Panel {
            Waveform(
                peaks = loaded.peaks,
                channels = loaded.analysis.channels,
                frames = loaded.analysis.frames,
                region = plan?.let { it.regionStart..it.regionEnd },
                modifier = Modifier
                    .fillMaxWidth()
                    .height(150.dp),
            )
        }

        Facts(loaded.analysis)
        if (plan != null) PlanCard(plan)

        SectionTitle("Loop")
        BarsRow(settings, onChange)
        SkipRow(settings, onChange)
        WorkflowRow(settings, onChange)

        SectionTitle("Varispeed")
        SpeedControls(settings, onChange)

        SectionTitle("Tape")
        TapeControls(settings, onChange)

        SectionTitle("Output")
        DepthRow(settings, onChange)
        Toggle("Normalize", settings.normalize) { on -> onChange { it.copy(normalize = on) } }
        Toggle("Snap to a sample-exact tempo", settings.snap) { on -> onChange { it.copy(snap = on) } }
        Toggle("Accept a short loop", settings.allowShort) { on -> onChange { it.copy(allowShort = on) } }

        Spacer(Modifier.height(8.dp))
        Button(
            onClick = onExport,
            enabled = busy is Busy.Idle && plan != null,
            modifier = Modifier.fillMaxWidth(),
        ) {
            Text(if (busy is Busy.Working) busy.what else "Export")
        }
        if (busy is Busy.Working) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                CircularProgressIndicator(Modifier.height(16.dp).width(16.dp), strokeWidth = 2.dp)
                Spacer(Modifier.width(8.dp))
                Text(busy.what, color = Palette.dim)
            }
        }
        Spacer(Modifier.height(24.dp))
    }
}

@Composable
private fun Header(loaded: Loaded?, onOpen: () -> Unit) {
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text("LOOP_SLCR", color = Palette.text, fontWeight = FontWeight.Bold, fontSize = 18.sp)
            Text(
                loaded?.name ?: "no file",
                color = Palette.dim,
                fontSize = 12.sp,
                fontFamily = FontFamily.Monospace,
            )
        }
        OutlinedButton(onClick = onOpen) { Text("Open") }
    }
}

@Composable
private fun Problem(message: String, onDismiss: () -> Unit) {
    Card(colors = CardDefaults.cardColors(containerColor = Color(0xFF2A1414))) {
        Column(Modifier.padding(12.dp)) {
            Text(message, color = Palette.bad, fontSize = 13.sp)
            Spacer(Modifier.height(4.dp))
            OutlinedButton(onClick = onDismiss) { Text("Dismiss") }
        }
    }
}

@Composable
private fun Panel(content: @Composable () -> Unit) {
    Card(
        colors = CardDefaults.cardColors(containerColor = Palette.surface),
        shape = RoundedCornerShape(8.dp),
    ) { content() }
}

@Composable
private fun SectionTitle(text: String) {
    Text(
        text.uppercase(),
        color = Palette.dim,
        fontSize = 11.sp,
        fontWeight = FontWeight.Bold,
        modifier = Modifier.padding(top = 8.dp),
    )
}

@Composable
private fun Facts(a: Analysis) {
    Panel {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Fact("format", "${a.channels} ch · ${a.sampleRate} Hz · ${a.bitsPerSample} bit")
            Fact("length", "${a.frames} frames · ${"%.3f".format(a.durationSeconds)} s")
            Fact("peak", "%.4f".format(a.peak))
            Fact("tempo", a.tempo?.let { trim(it) + " BPM" } ?: "unknown")
            Fact("loop", a.loopBars?.let { "$it bars (${a.workflow})" } ?: "unclear")
            if (a.tailFrames > 0) Fact("tail", "${a.tailFrames} frames below the floor")
        }
    }
}

@Composable
private fun PlanCard(p: Plan) {
    Panel {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Fact("cut", "${p.bars} bars from ${p.regionStart} (${p.barsSource})")
            Fact("workflow", "${p.workflowChosen} (detected ${p.workflowDetected})")
            Fact(
                "speed",
                if (p.ratio == 1.0) {
                    "unchanged"
                } else {
                    "%.6f".format(p.ratio) + " · " + "%.3f".format(p.semitones) + " st" +
                        (if (p.ratioExact) " · exact" else " · approximated")
                },
            )
            Fact("result", "${trim(p.resultingTempo)} BPM · ${p.outputFrames} frames")
            Fact(
                "peak",
                "%.4f".format(p.peak) + if (p.clips) "  — clips" else "",
                if (p.clips) Palette.bad else Palette.text,
            )
            if (p.shortBy > 0) {
                Fact("short by", "${p.shortBy} frames — the loop would drift", Palette.warn)
            }
            if (p.normalizeGain != null) Fact("normalize", "×" + "%.4f".format(p.normalizeGain))
            if (p.dithered) Fact("dither", "applied")
            if (p.tape) Fact("tape", "on")
        }
    }
}

@Composable
private fun Fact(label: String, value: String, colour: Color = Palette.text) {
    Row {
        Text(
            label,
            color = Palette.dim,
            fontSize = 12.sp,
            fontFamily = FontFamily.Monospace,
            modifier = Modifier.width(88.dp),
        )
        Text(value, color = colour, fontSize = 12.sp, fontFamily = FontFamily.Monospace)
    }
}

@Composable
private fun BarsRow(s: Settings, onChange: ((Settings) -> Settings) -> Unit) {
    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        Chip("auto", s.bars == null) { onChange { it.copy(bars = null) } }
        for (n in listOf(1L, 2L, 4L, 8L, 16L)) {
            Chip("$n", s.bars == n) { onChange { it.copy(bars = n) } }
        }
    }
}

@Composable
private fun SkipRow(s: Settings, onChange: ((Settings) -> Settings) -> Unit) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text("skip", color = Palette.dim, fontSize = 12.sp, modifier = Modifier.width(88.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            Chip("auto", s.skip == null) { onChange { it.copy(skip = null) } }
            for (n in listOf(0L, 1L, 2L, 4L)) {
                Chip("$n", s.skip == n) { onChange { it.copy(skip = n) } }
            }
        }
    }
}

@Composable
private fun WorkflowRow(s: Settings, onChange: ((Settings) -> Settings) -> Unit) {
    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        for (w in listOf("auto", "warmup", "foldback")) {
            Chip(w, s.workflow == w) { onChange { it.copy(workflow = w) } }
        }
    }
}

@Composable
private fun DepthRow(s: Settings, onChange: ((Settings) -> Settings) -> Unit) {
    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        for (d in listOf("16", "24", "32", "32f")) {
            Chip(d, s.depth == d) { onChange { it.copy(depth = d) } }
        }
    }
}

@Composable
private fun SpeedControls(s: Settings, onChange: ((Settings) -> Settings) -> Unit) {
    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        Chip("semitones", s.speedMode == SpeedMode.Semitones) {
            onChange { it.copy(speedMode = SpeedMode.Semitones) }
        }
        Chip("target BPM", s.speedMode == SpeedMode.TargetBpm) {
            onChange { it.copy(speedMode = SpeedMode.TargetBpm) }
        }
    }

    when (s.speedMode) {
        SpeedMode.Semitones -> {
            Fact("pitch", "%+.2f st".format(s.semitones))
            Slider(
                value = s.semitones.toFloat(),
                onValueChange = { raw ->
                    // A detent at unity, because "no change" has to be reachable
                    // with a finger. Without it every drag leaves a ratio of
                    // 1.003 that costs a resample and buys nothing.
                    val v = raw.toDouble()
                    val snapped = if (abs(v) < DETENT_SEMITONES) 0.0 else (v * 100).roundToInt() / 100.0
                    onChange { it.copy(semitones = snapped) }
                },
                valueRange = -12f..12f,
            )
        }
        SpeedMode.TargetBpm -> {
            var text by remember(s.speedMode) { mutableStateOf(s.targetBpm?.let(::trim) ?: "") }
            OutlinedTextField(
                value = text,
                onValueChange = { entered ->
                    text = entered
                    val value = entered.toDoubleOrNull()
                    onChange { it.copy(targetBpm = if (value != null && value > 0) value else null) }
                },
                label = { Text("target BPM") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}

@Composable
private fun TapeControls(s: Settings, onChange: ((Settings) -> Settings) -> Unit) {
    Toggle("Tape character", s.tape) { on -> onChange { it.copy(tape = on) } }
    if (s.tape) {
        Fact("wow", "%.2f %%".format(s.wow))
        Slider(
            value = s.wow.toFloat(),
            onValueChange = { v -> onChange { it.copy(wow = (v * 100).roundToInt() / 100.0) } },
            valueRange = 0f..2f,
        )
        Fact("flutter", "%.2f %%".format(s.flutter))
        Slider(
            value = s.flutter.toFloat(),
            onValueChange = { v -> onChange { it.copy(flutter = (v * 100).roundToInt() / 100.0) } },
            valueRange = 0f..2f,
        )
    }
}

@Composable
private fun Toggle(label: String, on: Boolean, onToggle: (Boolean) -> Unit) {
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, color = Palette.text, fontSize = 14.sp)
        Switch(checked = on, onCheckedChange = onToggle)
    }
}

@Composable
private fun Chip(label: String, selected: Boolean, onClick: () -> Unit) {
    FilterChip(selected = selected, onClick = onClick, label = { Text(label, fontSize = 12.sp) })
}
