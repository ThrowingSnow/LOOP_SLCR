package org.loopslcr.app

import androidx.compose.foundation.background
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
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
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
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
    motion: MotionSettings = MotionSettings(),
    onMotion: ((MotionSettings) -> MotionSettings) -> Unit = {},
    pair: PairSettings = PairSettings(),
    hasSecond: Boolean = false,
    onPair: ((PairSettings) -> PairSettings) -> Unit = {},
    /** Whether this loop is the pair's speed reference; null with no partner. */
    master: Boolean? = null,
    onMaster: (Boolean) -> Unit = {},
) {
    // One lane or two, and it is a *view*, not a setting: nothing about the file
    // or the cut changes. Folded, the picture is half as tall, and on a phone
    // that half is the difference between reading the plan and scrolling for it.
    var folded by rememberSaveable { mutableStateOf(false) }
    // Two parts, and the split is the point.
    //
    // **What you are looking at stays put.** The waveform and the varispeed are
    // pinned; everything that only *describes* the cut scrolls beside or beneath
    // them. The whole screen used to scroll as one, so reaching any control
    // pushed the picture it acted on off the top — you could change the thing or
    // watch the thing, never both. Folding the groups helped and did not fix it,
    // because a long enough list still scrolls the head away.
    //
    // **Sideways it is the same split turned ninety degrees**: the picture takes
    // the left half and the panels stand beside it. Measured rather than asked —
    // `maxWidth > maxHeight` is the question the layout actually has, and it is
    // also true of a tablet held upright with room to spare, which the device's
    // idea of "landscape" would have got wrong.
    BoxWithConstraints(
        Modifier
            .fillMaxSize()
            .background(Palette.background),
    ) {
        val wide = maxWidth > maxHeight

    @Composable
    fun ColumnScope.head() {
        Column(
            Modifier.padding(start = 12.dp, end = 12.dp, top = 10.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Header(
                loaded,
                busy,
                playing,
                onOpen,
                onPlay,
                folded = folded,
                onFold = { folded = !folded },
            )

            if (problem != null) Problem(problem, onDismissProblem)

            if (loaded != null) {
                Panel {
                    Waveform(
                        peaks = loaded.peaks,
                        channels = loaded.analysis.channels,
                        frames = loaded.analysis.frames,
                        region = plan?.let { it.regionStart..it.regionEnd },
                        playHead = playHead,
                        samplesPerBar = plan?.samplesPerBar,
                        onDrag = onDragMarker,
                        folded = folded,
                        modifier = Modifier
                            .fillMaxWidth()
                            .height(if (folded) 84.dp else 150.dp)
                            .testTag("wave"),
                    )
                }

                // The button went to the top; the sentence stays with the
                // waveform it describes.
                Text(
                    if (playing) {
                        "the loop is playing — move the varispeed and it bends"
                    } else {
                        "drag a marker to move the cut — it snaps to bar lines"
                    },
                    color = Palette.dim,
                    fontSize = 11.sp,
                )

                PitchStrip(
                    settings,
                    plan,
                    loaded.analysis,
                    onChange,
                    master = master,
                    onMaster = onMaster,
                )
            }
        }
    }

    @Composable
    fun ColumnScope.body(modifier: Modifier) {
        if (loaded == null) {
            Column(Modifier.padding(16.dp)) {
                Spacer(Modifier.height(24.dp))
                Text(
                    "Pick a WAVE file to cut. Nothing is read until you do, and " +
                        "nothing outside it is ever read.",
                    color = Palette.dim,
                )
            }
            return
        }

        Column(
            modifier
                .testTag("panels")
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 12.dp, vertical = 10.dp),
            // Tighter than the pinned half above it: these are lids in a list,
            // and air between lids only costs the list its last row.
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            // The file's own figures live behind the name in [Header]; what is
            // here is what a *decision* is made from.
            if (plan != null) PlanCard(plan, settings, onChange)

            Section(
                "Motion",
                summary = if (motion.on) {
                    "${motion.gridName(settings.sig)} · every " +
                        "${motion.rateName(settings.sig)} · ±${motion.depth} · " +
                        motion.shape.name.lowercase()
                } else {
                    "off"
                },
            ) {
                MotionControls(motion, settings, plan, onMotion)
            }

            Section(
                "Swap",
                summary = when {
                    !hasSecond -> "no second loop"
                    pair.on -> "${pair.holdA} on 1 · ${pair.holdB} on 2 · " +
                        "per ${pair.gridName(settings.sig)}"
                    else -> "off"
                },
            ) {
                PairControls(pair, settings, plan, hasSecond, onPair)
            }

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

        if (wide) {
            Row(Modifier.fillMaxSize()) {
                Column(Modifier.weight(1f)) { head() }
                Column(Modifier.weight(1f)) { body(Modifier.fillMaxHeight()) }
            }
        } else {
            Column(Modifier.fillMaxSize()) {
                head()
                body(Modifier.weight(1f))
            }
        }
    }
}

@Composable
private fun Header(
    loaded: Loaded?,
    busy: Busy,
    playing: Boolean,
    onOpen: () -> Unit,
    onPlay: () -> Unit,
    folded: Boolean = false,
    onFold: () -> Unit = {},
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
                        Caret(open)
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
                // Beside Play, because it is the other thing you reach for while
                // looking at the picture rather than at the numbers.
                Text(
                    if (folded) "1ch" else "2ch",
                    color = Palette.dim,
                    fontSize = 12.sp,
                    fontFamily = FontFamily.Monospace,
                    modifier = Modifier
                        .clickable { onFold() }
                        .padding(horizontal = 10.dp, vertical = 8.dp)
                        .testTag("lanes"),
                )
                Spacer(Modifier.width(4.dp))
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
internal fun Panel(content: @Composable () -> Unit) {
    Card(
        colors = CardDefaults.cardColors(containerColor = Palette.surface),
        border = BorderStroke(1.dp, Palette.outlineIdle),
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
internal fun Section(
    title: String,
    initiallyOpen: Boolean = false,
    summary: String? = null,
    content: @Composable () -> Unit,
) {
    var open by rememberSaveable(title) { mutableStateOf(initiallyOpen) }

    // An outline, because a column of unboxed rows gives the eye nothing to tell
    // one group of controls from the next — the settings all read as one list
    // and the headings look like labels rather than lids.
    Card(
        colors = CardDefaults.cardColors(containerColor = Palette.background),
        border = BorderStroke(1.dp, if (open) Palette.outline else Palette.outlineIdle),
        shape = RoundedCornerShape(8.dp),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(horizontal = 12.dp, vertical = 2.dp)) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .clickable { open = !open }
                    // A bigger target as well as a bigger mark: the whole row is
                    // tappable, and it stays tall enough to hit without aiming.
                    // 6 dp of padding around a 17 sp caret is still a ~40 dp row,
                    // which is the floor — below that this stops being a button.
                    .padding(vertical = 6.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    title.uppercase(),
                    color = Palette.dim,
                    fontSize = 11.sp,
                    fontWeight = FontWeight.Bold,
                )
                Caret(open)
                if (!open && summary != null) {
                    Text(
                        "   $summary",
                        color = Palette.dim,
                        fontSize = 11.sp,
                        fontFamily = FontFamily.Monospace,
                    )
                }
            }

            if (open) {
                Column(
                    Modifier.padding(bottom = 8.dp),
                    verticalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    content()
                }
            }
        }
    }
}

/**
 * A slider with a thinner track than Material's default.
 *
 * The stock one is a fat rounded bar — it reads as a progress meter, something
 * being reported to you, rather than as a control you hold. Slimming the track
 * and the thumb also buys back vertical room on a screen whose whole point is
 * that the waveform above it never has to move.
 *
 * One place, so the pitch, the tempo, the wow and the flutter cannot drift into
 * looking like four different kinds of control.
 */
// The `thumb` and `track` slots are still marked experimental. Taken knowingly:
// the alternative is Material's default bar, which reads as a progress meter
// rather than a control, and this is a slider — the least load-bearing API in
// the app to be pinned on.
@OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)
@Composable
internal fun ThinSlider(
    value: Float,
    onValueChange: (Float) -> Unit,
    valueRange: ClosedFloatingPointRange<Float>,
    modifier: Modifier = Modifier,
) {
    Slider(
        value = value,
        onValueChange = onValueChange,
        valueRange = valueRange,
        modifier = modifier.height(28.dp),
        thumb = {
            Box(
                Modifier
                    .size(width = 6.dp, height = 20.dp)
                    .background(Palette.wave, RoundedCornerShape(3.dp)),
            )
        },
        track = { state ->
            val fraction = if (valueRange.endInclusive > valueRange.start) {
                (state.value - valueRange.start) /
                    (valueRange.endInclusive - valueRange.start)
            } else {
                0f
            }
            Box(Modifier.fillMaxWidth().height(4.dp)) {
                Box(
                    Modifier
                        .fillMaxWidth()
                        .height(4.dp)
                        .background(Palette.trackIdle, RoundedCornerShape(2.dp)),
                )
                Box(
                    Modifier
                        .fillMaxWidth(fraction.coerceIn(0f, 1f))
                        .height(4.dp)
                        .background(Palette.wave, RoundedCornerShape(2.dp)),
                )
            }
        },
    )
}

/**
 * The fold marker.
 *
 * One place, so every triangle in the app is the same size and the same colour.
 * It was set in the body text size, which made it a punctuation mark rather than
 * a control — on a phone it read as a full stop that happened to be pointy.
 */
@Composable
private fun Caret(open: Boolean) {
    Text(
        if (open) "  ▴" else "  ▾",
        color = Palette.text,
        fontSize = 17.sp,
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
                    "${p.bars} bars · ${trim(p.resultingTempo)} BPM · ${p.outputFrames} frames",
                    color = Palette.text,
                    fontSize = 12.sp,
                    fontFamily = FontFamily.Monospace,
                )
                Caret(open = false)
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
                    "PLAN",
                    color = Palette.dim,
                    fontSize = 11.sp,
                    fontWeight = FontWeight.Bold,
                )
                Caret(open = true)
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
internal fun Fact(label: String, value: String, colour: Color = Palette.text) {
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
internal fun SkipRow(s: Settings, onChange: ((Settings) -> Settings) -> Unit) {
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
internal fun SourceControls(
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
internal fun WorkflowRow(s: Settings, plan: Plan?, onChange: ((Settings) -> Settings) -> Unit) {
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
internal fun PitchStrip(
    s: Settings,
    plan: Plan?,
    analysis: Analysis,
    onChange: ((Settings) -> Settings) -> Unit,
    /**
     * Whether *this* loop is the one the pair's speed is measured against —
     * or null when there is no second loop and the question does not arise.
     */
    master: Boolean? = null,
    onMaster: (Boolean) -> Unit = {},
) {
    // The source tempo, which is what a target tempo is a ratio *of*. Without
    // it there is no honest slider range, only an invented one.
    //
    // Falls back to the file's own tempo rather than the plan's alone. The plan
    // is null before the first one lands and again whenever the settings do not
    // describe a cut, and the slider used to simply not be there in those
    // moments — no slider, no reason given, which is exactly the kind of silent
    // disappearance this app keeps having to be talked out of.
    val source = plan?.tempo ?: analysis.tempo

    // Typed as well as swept: a slider cannot land on exactly 90, and exactly 90
    // is usually the point.
    var typed by remember { mutableStateOf(s.targetBpm?.let(::trim) ?: "") }
    // Adopt a value that arrived from somewhere else — the slider, or the
    // calculator's "send to cutter". Keyed on the *value*, not on the mode:
    // keying on the mode meant a tempo sent from the calculator while this mode
    // was already selected left the old number sitting in the field, which is
    // how it looked like nothing had happened.
    LaunchedEffect(s.targetBpm) {
        val mine = typed.toDoubleOrNull()
        if (s.targetBpm != null && s.targetBpm != mine) typed = trim(s.targetBpm)
    }

    // One row for both units: each mode's button, and immediately behind it the
    // number that mode produces. They were a button row and a readout row, which
    // spent a whole line of a screen whose entire layout exists to keep the
    // waveform in sight — and put the number you type a slider away from the
    // number it makes.
    //
    // Both readouts show at all times. The dim one is derived, the bright one is
    // what you are driving; seeing the other unit move while you drag is most of
    // the reason to have two units at all.
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Chip("semitones", s.speedMode == SpeedMode.Semitones) {
            onChange { it.copy(speedMode = SpeedMode.Semitones) }
        }
        Spacer(Modifier.width(6.dp))
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
            shown?.let { "%+.2f st".format(it) } ?: "—",
            color = when {
                shown == null -> Palette.dim
                s.speedMode != SpeedMode.Semitones -> Palette.dim
                shown == 0.0 -> Palette.dim
                else -> Palette.text
            },
            fontSize = 12.sp,
            fontFamily = FontFamily.Monospace,
            maxLines = 1,
        )

        Spacer(Modifier.weight(1f))

        Chip("target BPM", s.speedMode == SpeedMode.TargetBpm) {
            onChange { it.copy(speedMode = SpeedMode.TargetBpm) }
        }
        Spacer(Modifier.width(6.dp))
        if (s.speedMode == SpeedMode.TargetBpm) {
            TempoField(
                value = typed,
                placeholder = source?.let(::trim) ?: "—",
                onValueChange = { entered ->
                    typed = entered
                    val value = entered.toDoubleOrNull()
                    onChange { it.copy(targetBpm = if (value != null && value > 0) value else null) }
                },
                modifier = Modifier.width(74.dp).testTag("tempoField"),
            )
        } else {
            // Not editable in the other mode, but not missing either: the tempo
            // the cut will land on is the whole point of moving the semitones,
            // and it belongs beside the button that would let you set it.
            Text(
                plan?.resultingTempo?.let { trim(it) } ?: source?.let(::trim) ?: "—",
                color = Palette.dim,
                fontSize = 12.sp,
                fontFamily = FontFamily.Monospace,
                maxLines = 1,
                modifier = Modifier.width(74.dp),
                textAlign = TextAlign.End,
            )
        }
    }

    if (plan != null && plan.ratio != 1.0 && !plan.ratioExact) {
        // Only when it is *not* exact. The resulting tempo is on the row above;
        // what that row cannot say is that the ratio did not come out clean, and
        // a warning is the one thing folding may never hide.
        Text(
            "${trim(plan.tempo)} → ${trim(plan.resultingTempo)} BPM, not exact",
            color = Palette.warn,
            fontSize = 11.sp,
            fontFamily = FontFamily.Monospace,
        )
    }

    when (s.speedMode) {
        SpeedMode.Semitones -> SpeedRow(master, onMaster) {
            ThinSlider(
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
                modifier = Modifier.weight(1f).testTag("semitoneSlider"),
            )
        }

        SpeedMode.TargetBpm -> {
            // The same musical span as the semitone slider: an octave either
            // way. Deriving it from the source rather than picking absolute
            // numbers means the two controls always cover the same ground, and
            // the slider cannot reach a ratio the preview would have to clamp.
            if (source != null && source > 0.0) {
                val low = (source / 2.0).toFloat()
                val high = (source * 2.0).toFloat()
                SpeedRow(master, onMaster) {
                    ThinSlider(
                        value = (s.targetBpm ?: source).toFloat().coerceIn(low, high),
                        onValueChange = { raw ->
                            val snapped = (raw * 1000).roundToInt() / 1000.0
                            // A detent at the source tempo, for the same reason the
                            // semitone slider has one at zero.
                            val v = if (abs(snapped - source) < source * 0.002) source else snapped
                            onChange { it.copy(targetBpm = v) }
                        },
                        valueRange = low..high,
                        modifier = Modifier.weight(1f).testTag("bpmSlider"),
                    )
                }
            }

            if (source == null || source <= 0.0) {
                // Said, not hidden. Fourteen of the 279 archive files carry no
                // tempo anywhere; without one, "half as fast" has nothing to be
                // half of, and a slider spanning invented numbers would be a
                // worse answer than none.
                Text(
                    "no source tempo yet — type one under SOURCE and the slider appears",
                    color = Palette.warn,
                    fontSize = 11.sp,
                )
            }

        }
    }
}

/**
 * The speed slider, with the MSTR switch beside it.
 *
 * # What MSTR means
 *
 * There is one speed in the pair, because there is one play head. What the
 * switch chooses is which loop that speed is *measured against*: on, and the
 * pair runs at this loop's own tempo, so the other one is pulled to it.
 *
 * **Only one of the two cutters can have it on.** Turning it on here turns it
 * off over there — two loops both claiming to be the reference is not a state
 * that means anything, so it is not a state that can be reached.
 *
 * # Why it releases itself
 *
 * It is a mode, and a mode holding a value has to be told when to let go. This
 * one lets go the moment the speed is moved by hand: drag the slider, type a
 * tempo, switch units, and MSTR goes out. Anything else would have the switch
 * quietly putting the tempo back after every drag — the control fighting the
 * finger, which is the reason this was two buttons before.
 *
 * Absent, not greyed, when there is no second loop: with one loop there is
 * nothing to be master *of*.
 */
@Composable
private fun SpeedRow(
    master: Boolean?,
    onMaster: (Boolean) -> Unit,
    slider: @Composable RowScope.() -> Unit,
) {
    if (master == null) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) { slider() }
        return
    }
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Chip("MSTR", master, Modifier.testTag("master")) { onMaster(!master) }
        Spacer(Modifier.width(8.dp))
        slider()
    }
}

/**
 * The stepped displacement of the play head.
 *
 * # What it does
 *
 * The loop is divided into equal pieces — a bar, a beat, half a beat — and on
 * each piece boundary the play head is displaced by a whole number of pieces.
 * Beats get rearranged into beats. Because the grid *is* the loop divided, and
 * because the displacement is a function of which piece you are in rather than
 * of elapsed time, the result still repeats exactly once per loop.
 *
 * # What it deliberately does not do
 *
 * **It never reaches the file.** This is a way of listening to the cut, not a
 * stage in making it, and Export writes the same bytes whether it is on or off.
 * That is said on the panel rather than left to be discovered, because a control
 * that looks like the others and quietly is not one is worse than no control.
 */
@Composable
private fun MotionControls(
    m: MotionSettings,
    s: Settings,
    plan: Plan?,
    onMotion: ((MotionSettings) -> MotionSettings) -> Unit,
) {
    Toggle("Move the play head", m.on) { on -> onMotion { it.copy(on = on) } }

    Text(
        "The loop is cut into pieces and played out of order — always landing " +
            "on a piece, so it stays a loop. It changes what you hear, never " +
            "what Export writes.",
        color = Palette.dim,
        fontSize = 11.sp,
    )

    Row(verticalAlignment = Alignment.CenterVertically) {
        Text("grid", color = Palette.dim, fontSize = 11.sp, fontFamily = FontFamily.Monospace)
        Spacer(Modifier.width(10.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            MotionSettings.divisions.forEach { per ->
                Chip(MotionSettings.gridName(per, s.sig), m.perBar == per) {
                    // The rate may not be finer than the grid — a move with no
                    // piece to land on is not a thing this can mean — so a
                    // coarser grid carries the rate along with it.
                    onMotion { it.copy(perBar = per, ratePerBar = it.ratePerBar.coerceAtMost(per)) }
                }
            }
        }
    }

    // How often, as opposed to where. One control did both to begin with, which
    // meant asking for half-beat landings also asked for a half-beat stutter:
    // the fine grid was unusable at any musical rate.
    //
    // Counted in the same per-bar unit as the grid, so there is no number here
    // that could fall between two beats. That is the whole of "beat-synced" —
    // not a setting, a unit.
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text("every", color = Palette.dim, fontSize = 11.sp, fontFamily = FontFamily.Monospace)
        Spacer(Modifier.width(4.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            MotionSettings.divisions.filter { it <= m.perBar }.forEach { rate ->
                Chip(MotionSettings.gridName(rate, s.sig), m.ratePerBar == rate) {
                    onMotion { it.copy(ratePerBar = rate) }
                }
            }
        }
    }

    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        MotionShape.entries.forEach { shape ->
            Chip(shape.name.lowercase(), m.shape == shape) {
                onMotion { it.copy(shape = shape) }
            }
        }
    }

    // The reach, in pieces. Bounded by the loop itself: a jump further than the
    // loop is long is the same jump wrapped round, so offering it would be
    // offering a control that stops doing anything past a point it does not
    // mark.
    val pieces = m.steps(plan?.bars)
    val most = ((pieces ?: 8) - 1).coerceIn(1, 16)
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Text(
            "reach   ±${m.depth} ${MotionSettings.gridName(m.perBar, s.sig)}" +
                if (m.depth == 1) "" else "s",
            color = Palette.dim,
            fontSize = 11.sp,
            fontFamily = FontFamily.Monospace,
        )
    }
    ThinSlider(
        value = m.depth.toFloat().coerceIn(1f, most.toFloat()),
        onValueChange = { raw -> onMotion { it.copy(depth = raw.roundToInt()) } },
        valueRange = 1f..most.toFloat(),
        modifier = Modifier.fillMaxWidth().testTag("motionDepth"),
    )

    if (pieces == null) {
        Text(
            "no loop yet — the grid is the loop divided up, so there is nothing " +
                "to divide until a plan lands",
            color = Palette.warn,
            fontSize = 11.sp,
        )
    }
}

/**
 * The alternation between the two loops.
 *
 * # What it does
 *
 * Holds the first loop for so many pieces of the grid, then the second for so
 * many, over and over. One play head serves both, so the second loop is heard at
 * the same place in the bar the first would have been — nothing is retriggered,
 * and there are no two clocks that could drift.
 *
 * # Where it can go wrong, and what is said about it
 *
 * The pair needs both loops to be exactly the same length. That is achievable
 * exactly, because both tempi are known: the second loop is cut to the first
 * one's bar count and pulled to its tempo with the same rational arithmetic as
 * any other cut. When it is *not* achievable — no tempo declared, a different
 * sample rate — the engine refuses rather than stretching, and the reason turns
 * up in CUTTER 2 with both numbers in it.
 *
 * A cycle that does not divide the loop is a different matter: the last turn
 * before the seam comes out short. That is a musical choice rather than a fault,
 * so it is said and not prevented.
 */
@Composable
private fun PairControls(
    p: PairSettings,
    s: Settings,
    plan: Plan?,
    hasSecond: Boolean,
    onPair: ((PairSettings) -> PairSettings) -> Unit,
) {
    if (!hasSecond) {
        Text(
            "Open a second loop under CUTTER 2. It is cut to this loop's bar " +
                "count and pulled to its tempo, so the two can share one play " +
                "head — which is what keeps them in time without either being " +
                "restarted.",
            color = Palette.dim,
            fontSize = 11.sp,
        )
        return
    }

    Toggle("Alternate between the two loops", p.on) { on -> onPair { it.copy(on = on) } }

    Row(verticalAlignment = Alignment.CenterVertically) {
        Text("per", color = Palette.dim, fontSize = 11.sp, fontFamily = FontFamily.Monospace)
        Spacer(Modifier.width(10.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            MotionSettings.divisions.forEach { per ->
                Chip(MotionSettings.gridName(per, s.sig), p.perBar == per) {
                    onPair { it.copy(perBar = per) }
                }
            }
        }
    }

    Text(
        "hold   ${p.holdA} on loop 1, then ${p.holdB} on loop 2",
        color = Palette.dim,
        fontSize = 11.sp,
        fontFamily = FontFamily.Monospace,
    )
    ThinSlider(
        value = p.holdA.toFloat().coerceIn(1f, 16f),
        onValueChange = { raw -> onPair { it.copy(holdA = raw.roundToInt()) } },
        valueRange = 1f..16f,
        modifier = Modifier.fillMaxWidth().testTag("holdA"),
    )
    ThinSlider(
        value = p.holdB.toFloat().coerceIn(0f, 16f),
        onValueChange = { raw -> onPair { it.copy(holdB = raw.roundToInt()) } },
        valueRange = 0f..16f,
        modifier = Modifier.fillMaxWidth().testTag("holdB"),
    )

    if (p.holdB == 0) {
        Text(
            "nothing on loop 2 — it never comes in",
            color = Palette.dim,
            fontSize = 11.sp,
        )
    }

    if (!p.fitsTheLoop(plan?.bars)) {
        // Said, not prevented. The loop still repeats exactly — the count
        // restarts with it — but one turn of the alternation is shorter than
        // the others, and that is worth knowing before it is blamed on a bug.
        Text(
            "${p.holdA + p.holdB} does not divide " +
                "${p.steps(plan?.bars) ?: "the loop"} — the last turn before " +
                "the seam comes out short. Still a loop; just an uneven one.",
            color = Palette.warn,
            fontSize = 11.sp,
        )
    }
}

/**
 * The typed tempo: one row, a quarter wide, beside the speed it sets.
 *
 * Hand-rolled rather than [OutlinedTextField], which has a fixed 56 dp minimum
 * and a floating label above that — most of a thumb's height of air around four
 * characters, on the one screen whose entire layout exists so the waveform never
 * has to move.
 *
 * The box is its own [Box] rather than the field's `modifier`, which
 * [BasicTextField] hands to the editor *inside* the decoration — so anything
 * measuring what the caller sized would be measuring the text, minus the
 * padding, and not the cell at all.
 */
@Composable
private fun TempoField(
    value: String,
    placeholder: String,
    onValueChange: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    Box(
        modifier
            .border(1.dp, Palette.outline, RoundedCornerShape(6.dp))
            .padding(horizontal = 8.dp, vertical = 6.dp),
    ) {
        BasicTextField(
            value = value,
            onValueChange = onValueChange,
            singleLine = true,
            textStyle = TextStyle(
                color = Palette.text,
                fontSize = 13.sp,
                fontFamily = FontFamily.Monospace,
                textAlign = TextAlign.End,
            ),
            cursorBrush = SolidColor(Palette.wave),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
            modifier = Modifier.fillMaxWidth(),
            decorationBox = { inner ->
                Row(verticalAlignment = Alignment.CenterVertically) {
                    // The unit inside the box rather than a label above it. The chip
                    // overhead already says target BPM; this only has to stop the
                    // number being a bare figure with no dimension.
                    Text(
                        "BPM",
                        color = Palette.dim,
                        fontSize = 10.sp,
                        fontFamily = FontFamily.Monospace,
                    )
                    Box(Modifier.weight(1f), contentAlignment = Alignment.CenterEnd) {
                        // The source tempo greyed out when nothing is typed, so an
                        // empty box still says what leaving it empty means.
                        if (value.isEmpty()) {
                            Text(
                                placeholder,
                                color = Palette.dim,
                                fontSize = 13.sp,
                                fontFamily = FontFamily.Monospace,
                            )
                        }
                        inner()
                    }
                }
            },
        )
    }
}


@Composable
internal fun TapeControls(s: Settings, onChange: ((Settings) -> Settings) -> Unit) {
    Toggle("Tape character", s.tape) { on -> onChange { it.copy(tape = on) } }
    if (s.tape) {
        Fact("wow", "%.2f %%".format(s.wow))
        ThinSlider(
            value = s.wow.toFloat(),
            onValueChange = { v -> onChange { it.copy(wow = (v * 100).roundToInt() / 100.0) } },
            valueRange = 0f..2f,
        )
        Fact("flutter", "%.2f %%".format(s.flutter))
        ThinSlider(
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
private fun Chip(
    label: String,
    selected: Boolean,
    modifier: Modifier = Modifier,
    onClick: () -> Unit,
) {
    FilterChip(
        selected = selected,
        onClick = onClick,
        label = { Text(label, fontSize = 12.sp) },
        modifier = modifier,
    )
}
