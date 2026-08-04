package org.loopslcr.app

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
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
    playing: Boolean = false,
    playHead: Float? = null,
    onOpen: () -> Unit,
    onExport: () -> Unit,
    onPlay: () -> Unit = {},
    onDragMarker: ((Marker, Float) -> Unit)? = null,
    onChange: ((Settings) -> Settings) -> Unit,
    onDismissProblem: () -> Unit,
) {
    Column(
        Modifier
            .fillMaxSize()
            .background(Palette.background)
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Header(loaded, busy, playing, onOpen, onPlay)

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
                playHead = playHead,
                samplesPerBar = plan?.samplesPerBar,
                onDrag = onDragMarker,
                modifier = Modifier
                    .fillMaxWidth()
                    .height(150.dp),
            )
        }

        // The button went to the top; the sentence stays with the waveform it
        // describes.
        Text(
            if (playing) {
                "the loop is playing — move the varispeed and it bends"
            } else {
                "drag a marker to move the cut — it snaps to bar lines"
            },
            color = Palette.dim,
            fontSize = 11.sp,
        )

        PitchStrip(settings, plan, onChange)

        // The file's own figures now live behind the name in [Header]; what
        // stays on screen is what a *decision* is made from. They were three
        // panels deep before the first control, and a screen you have to scroll
        // past to reach the thing you came for is a screen that buried it.
        if (plan != null) PlanCard(plan, settings, onChange)

        // Folded by default where the setting is normally read off the file and
        // left alone; open where the sliders are, because those are what the
        // preview is for.
        Section(
            "Source",
            summary = plan?.let { "${trim(it.tempo)} BPM · ${settings.sig}" },
        ) {
            SourceControls(loaded.analysis, settings, onChange)
        }

        Section(
            "Loop",
            summary = plan?.let { "${it.bars} bars from ${it.skipBars}" },
        ) {
            BarsRow(settings, onChange)
            SkipRow(settings, onChange)
            WorkflowRow(settings, plan, onChange)
            AlignRow(settings, onChange)
        }

        Section("Tape", summary = if (settings.tape) "on" else "off") {
            TapeControls(settings, onChange)
        }

        Section(
            "Output",
            summary = settings.depth + if (settings.normalize) " · normalized" else "",
        ) {
            DepthRow(settings, onChange)
            Toggle("Normalize", settings.normalize) { on -> onChange { it.copy(normalize = on) } }
            Toggle("Snap to a sample-exact tempo", settings.snap) { on -> onChange { it.copy(snap = on) } }
            Toggle("Accept a short loop", settings.allowShort) { on -> onChange { it.copy(allowShort = on) } }
        }

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
private fun Header(
    loaded: Loaded?,
    busy: Busy,
    playing: Boolean,
    onOpen: () -> Unit,
    onPlay: () -> Unit,
) {
    // Collapsed by default, and keyed on the file so a new one never opens
    // showing the last file's figures.
    var open by remember(loaded) { mutableStateOf(false) }

    Column(Modifier.fillMaxWidth()) {
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(
                Modifier
                    .weight(1f)
                    // The whole name is the target, not a separate icon: on a
                    // phone the name is already the biggest thing to hit.
                    .then(
                        if (loaded == null) {
                            Modifier
                        } else {
                            Modifier.clickable { open = !open }
                        },
                    ),
            ) {
                Text(
                    "LOOP_SLCR",
                    color = Palette.text,
                    fontWeight = FontWeight.Bold,
                    fontSize = 18.sp,
                )
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        loaded?.name ?: "no file",
                        color = Palette.dim,
                        fontSize = 12.sp,
                        fontFamily = FontFamily.Monospace,
                    )
                    if (loaded != null) {
                        Text(
                            if (open) "  ▴" else "  ▾",
                            color = Palette.dim,
                            fontSize = 12.sp,
                        )
                    }
                }
            }
            // The corner belongs to whatever gets pressed most, and that is not
            // Open: a file is chosen once and then listened to for minutes.
            // Play is also the control you reach for *while* looking at the
            // waveform, so it sits beside it rather than under it.
            if (loaded == null) {
                OutlinedButton(onClick = onOpen) { Text("Open") }
            } else {
                OutlinedButton(onClick = onPlay, enabled = busy is Busy.Idle) {
                    Text(if (playing) "Stop" else "Play")
                }
            }
        }

        if (loaded != null && open) {
            Spacer(Modifier.height(8.dp))
            Facts(loaded.analysis)
            Spacer(Modifier.height(8.dp))
            // Open lives in here now. Changing the file is a rare, destructive-
            // feeling act — it throws away every setting on the screen — and it
            // was sitting under the thumb next to nothing else.
            OutlinedButton(onClick = onOpen, modifier = Modifier.fillMaxWidth()) {
                Text("Open another file")
            }
        }
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

/**
 * A titled group that folds away.
 *
 * The screen is a single column and the varispeed sits five groups down it, so
 * reaching a slider while the loop is playing meant scrolling the waveform off
 * the top — the one thing you wanted to watch while you moved it. Folding the
 * groups you are not using brings the ones you are within a thumb's reach of the
 * picture.
 *
 * [rememberSaveable] rather than [remember]: a rotation or a trip through the
 * file picker must not silently reopen everything the user folded away.
 * [summary] shows on the collapsed header, so folding hides detail, never state.
 */
@Composable
private fun Section(
    title: String,
    initiallyOpen: Boolean = false,
    summary: String? = null,
    content: @Composable () -> Unit,
) {
    var open by rememberSaveable(title) { mutableStateOf(initiallyOpen) }

    Row(
        Modifier
            .fillMaxWidth()
            .clickable { open = !open }
            .padding(top = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            title.uppercase(),
            color = Palette.dim,
            fontSize = 11.sp,
            fontWeight = FontWeight.Bold,
        )
        Text(if (open) "  ▴" else "  ▾", color = Palette.dim, fontSize = 11.sp)
        if (!open && summary != null) {
            Text(
                "   $summary",
                color = Palette.dim,
                fontSize = 11.sp,
                fontFamily = FontFamily.Monospace,
            )
        }
    }

    if (open) content()
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
            // Which build this is. Printed rather than guessed at — see the
            // note on `versionName` in build.gradle.kts.
            Fact("build", BuildConfig.VERSION_NAME + " · engine " + Engine.version)
            if (a.tailFrames > 0) Fact("tail", "${a.tailFrames} frames below the floor")
        }
    }
}

@Composable
private fun PlanCard(p: Plan, s: Settings, onChange: ((Settings) -> Settings) -> Unit) {
    var open by rememberSaveable { mutableStateOf(false) }

    // **Folding must never hide a problem.** Everything in this card is detail
    // except the two things that say the cut is not what was asked for; those
    // stay on the collapsed line. A card that could swallow "clips" would be
    // worse than a card that does not fold.
    val trouble = when {
        p.clips -> "clips"
        p.shortBy > 0 -> "short by ${p.shortBy}"
        else -> null
    }

    if (!open) {
        Panel {
            Row(
                Modifier
                    .fillMaxWidth()
                    .clickable { open = true }
                    .padding(12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    "${p.bars} bars · ${trim(p.resultingTempo)} BPM · ${p.outputFrames} frames  ▾",
                    color = Palette.text,
                    fontSize = 12.sp,
                    fontFamily = FontFamily.Monospace,
                )
                if (trouble != null) {
                    Text(
                        "   $trouble",
                        color = if (p.clips) Palette.bad else Palette.warn,
                        fontSize = 12.sp,
                        fontFamily = FontFamily.Monospace,
                    )
                }
            }
        }
        return
    }

    Panel {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Row(
                Modifier.fillMaxWidth().clickable { open = false },
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    "PLAN  ▴",
                    color = Palette.dim,
                    fontSize = 11.sp,
                    fontWeight = FontWeight.Bold,
                )
            }
            Fact("cut", "${p.bars} bars from ${p.regionStart} (${p.barsSource})")
            Fact(
                "path",
                Paths.label(p.workflowChosen) +
                    " · detected " + Paths.label(p.workflowDetected) +
                    (p.audibleLoops?.let { " · %.2f loops audible".format(it) } ?: ""),
            )
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
                buildString {
                    append("%.4f".format(p.peak))
                    if (p.clips) append("  — clips")
                    // Said rather than implied: the dry run skips the resampling
                    // so it can answer while a finger is moving, and a
                    // resampler overshoots by a fraction of a dB.
                    if (p.peakBeforeVarispeed && p.ratio != 1.0) append("  (before varispeed)")
                },
                if (p.clips) Palette.bad else Palette.text,
            )
            if (p.shortBy > 0) {
                Fact("short by", "${p.shortBy} frames — the loop would drift", Palette.warn)
            }
            if (p.normalizeGain != null) Fact("normalize", "×" + "%.4f".format(p.normalizeGain))
            if (p.dithered) Fact("dither", "applied")
            if (p.tape) Fact("tape", "on")

            // A foldback adds the tail onto the head, so overshooting full
            // scale is normal rather than a mistake — the tool reports it
            // instead of clipping quietly. Reporting it without offering the
            // remedy leaves the user holding a number and no next move.
            if (p.clips && !s.normalize) {
                Spacer(Modifier.height(6.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        "the export would clip",
                        color = Palette.bad,
                        fontSize = 12.sp,
                        modifier = Modifier.weight(1f),
                    )
                    OutlinedButton(onClick = { onChange { it.copy(normalize = true) } }) {
                        Text("Normalize", fontSize = 12.sp)
                    }
                }
            }
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

/**
 * What the file is, where the file is wrong.
 *
 * The tempo field is blank by default and stays blank while the file knows its
 * own tempo — a prefilled box invites editing something that was already right.
 * It exists for the files that declare nothing, which without it cannot be cut
 * at all.
 */
@Composable
private fun SourceControls(
    a: Analysis,
    s: Settings,
    onChange: ((Settings) -> Settings) -> Unit,
) {
    var text by remember(a) { mutableStateOf(s.bpm) }
    OutlinedTextField(
        value = text,
        onValueChange = { entered ->
            text = entered
            onChange { it.copy(bpm = entered) }
        },
        label = { Text("BPM") },
        // Supporting text rather than a placeholder: Material only shows a
        // placeholder once the field has focus, and "what does the file say"
        // is precisely what you want to know *before* deciding to type.
        supportingText = {
            Text(
                when {
                    text.trim().toDoubleOrNull() != null -> "overriding the file"
                    a.tempo != null -> "blank — using ${trim(a.tempo)} from the file"
                    else -> "the file declares no tempo; enter one to cut it"
                },
                fontSize = 11.sp,
            )
        },
        isError = a.tempo == null && text.trim().toDoubleOrNull() == null,
        singleLine = true,
        modifier = Modifier.fillMaxWidth(),
    )
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text("signature", color = Palette.dim, fontSize = 12.sp, modifier = Modifier.width(88.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            for (v in listOf("4/4", "3/4", "6/8", "7/8")) {
                Chip(v, s.sig == v) { onChange { it.copy(sig = v) } }
            }
        }
    }
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text("BPM unit", color = Palette.dim, fontSize = 12.sp, modifier = Modifier.width(88.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            for (v in listOf("1/4", "3/8", "1/8")) {
                Chip(v, s.bpmUnit == v) { onChange { it.copy(bpmUnit = v) } }
            }
        }
    }
}

@Composable
private fun AlignRow(s: Settings, onChange: ((Settings) -> Settings) -> Unit) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text("align", color = Palette.dim, fontSize = 12.sp, modifier = Modifier.width(88.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            Chip("loop", s.align == "loop") { onChange { it.copy(align = "loop") } }
            Chip("grid", s.align == "grid") { onChange { it.copy(align = "grid") } }
        }
    }
    Text(
        if (s.align == "loop") {
            "every repeat the same length"
        } else {
            "both markers on bar lines; length may vary by a sample"
        },
        color = Palette.dim,
        fontSize = 11.sp,
    )
}

/**
 * Which of the two paths to take, and why.
 *
 * The chips alone were the whole control before, labelled in the engine's words
 * and with the detection's answer buried in the plan card. Three chips is still
 * the right control — what was missing is that a choice made for you has to be
 * visible, explicable and refusable. See [Paths].
 */
@Composable
private fun WorkflowRow(s: Settings, plan: Plan?, onChange: ((Settings) -> Settings) -> Unit) {
    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        for (w in Paths.offered) {
            Chip(Paths.label(w), s.workflow == w) { onChange { it.copy(workflow = w) } }
        }
    }

    if (plan != null) {
        val note = Paths.note(s.workflow, plan)
        Text(
            note.text,
            color = if (note.warn) Palette.warn else Palette.dim,
            fontSize = 11.sp,
            modifier = Modifier.padding(top = 4.dp),
        )
        // What the path actually does, described for the one that will run —
        // which under `auto` is the detected one, not the word "auto".
        Text(
            Paths.explain(plan.workflowChosen),
            color = Palette.dim,
            fontSize = 11.sp,
        )
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

/**
 * The pitch slider, directly under the waveform.
 *
 * It is the one control held *while listening*, and it was five folded groups
 * down a scrolling column — so using it pushed the picture it acts on off the
 * top of the screen. Everything else about the varispeed (the mode, the three
 * unit readouts, the target tempo field) stays in its section, because those are
 * read rather than held.
 *
 * Only in semitone mode. A target tempo is typed, not swept, and a slider that
 * appeared and vanished under the waveform would be worse than one that sits
 * where it belongs.
 */
@Composable
private fun PitchStrip(s: Settings, plan: Plan?, onChange: ((Settings) -> Settings) -> Unit) {
    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        Chip("semitones", s.speedMode == SpeedMode.Semitones) {
            onChange { it.copy(speedMode = SpeedMode.Semitones) }
        }
        Chip("target BPM", s.speedMode == SpeedMode.TargetBpm) {
            onChange { it.copy(speedMode = SpeedMode.TargetBpm) }
        }
    }

    // The source tempo, which is what a target tempo is a ratio *of*. Without
    // it there is no honest slider range, only an invented one.
    val source = plan?.tempo

    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Text("speed", color = Palette.dim, fontSize = 11.sp, fontFamily = FontFamily.Monospace)
        // In semitone mode this is the *setting*, not the plan. Same lesson the
        // markers taught: a live control has to follow the finger, and reading
        // it back from the pipeline puts a debounce between a drag and its own
        // readout. In target-BPM mode there is no setting to show — the
        // semitones are derived — so the plan is the only source.
        val shown = when (s.speedMode) {
            SpeedMode.Semitones -> s.semitones
            SpeedMode.TargetBpm -> plan?.semitones
        }
        Text(
            shown?.let { "  %+.2f st".format(it) } ?: "  —",
            color = if (shown == null || shown == 0.0) Palette.dim else Palette.text,
            fontSize = 12.sp,
            fontFamily = FontFamily.Monospace,
        )
        if (plan != null && plan.ratio != 1.0) {
            Text(
                "   ${trim(plan.tempo)} → ${trim(plan.resultingTempo)} BPM",
                color = if (plan.ratioExact) Palette.dim else Palette.warn,
                fontSize = 11.sp,
                fontFamily = FontFamily.Monospace,
            )
        }
    }

    when (s.speedMode) {
        SpeedMode.Semitones -> Slider(
            value = s.semitones.toFloat().coerceIn(-12f, 12f),
            onValueChange = { raw ->
                // A detent at unity, because "no change" has to be reachable
                // with a finger. Without it every drag leaves a ratio of 1.003
                // that costs a resample and buys nothing.
                val v = raw.toDouble()
                val snapped = if (abs(v) < DETENT_SEMITONES) 0.0 else (v * 100).roundToInt() / 100.0
                onChange { it.copy(semitones = snapped) }
            },
            valueRange = -12f..12f,
            modifier = Modifier.fillMaxWidth(),
        )

        SpeedMode.TargetBpm -> {
            // The same musical span as the semitone slider: an octave either
            // way. Deriving it from the source rather than picking absolute
            // numbers means the two controls always cover the same ground, and
            // the slider cannot reach a ratio the preview would have to clamp.
            if (source != null && source > 0.0) {
                val low = (source / 2.0).toFloat()
                val high = (source * 2.0).toFloat()
                Slider(
                    value = (s.targetBpm ?: source).toFloat().coerceIn(low, high),
                    onValueChange = { raw ->
                        val snapped = (raw * 1000).roundToInt() / 1000.0
                        // A detent at the source tempo, for the same reason the
                        // semitone slider has one at zero.
                        val v = if (abs(snapped - source) < source * 0.002) source else snapped
                        onChange { it.copy(targetBpm = v) }
                    },
                    valueRange = low..high,
                    modifier = Modifier.fillMaxWidth(),
                )
            }

            // Typed as well as swept: a slider cannot land on exactly 90, and
            // exactly 90 is usually the point.
            var text by remember { mutableStateOf(s.targetBpm?.let(::trim) ?: "") }
            // Adopt a value that arrived from somewhere else — the slider, or
            // the calculator's "send to cutter". Keyed on the *value*, not on
            // the mode: keying on the mode meant a tempo sent from the
            // calculator while this mode was already selected left the old
            // number sitting in the field, which is how it looked like nothing
            // had happened.
            LaunchedEffect(s.targetBpm) {
                val mine = text.toDoubleOrNull()
                if (s.targetBpm != null && s.targetBpm != mine) text = trim(s.targetBpm)
            }
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
