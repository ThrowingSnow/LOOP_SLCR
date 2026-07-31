package org.loopslcr.app

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
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
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.FilterChip
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * The calculator tab.
 *
 * Every number on it came from Rust. The screen's only jobs are asking for a
 * tempo and laying the answers out — see [Calculator] for why that division is
 * not negotiable.
 */
@Composable
fun CalculatorScreen(
    settings: CalculatorSettings,
    sums: Sums?,
    problem: String?,
    onSendToCutter: () -> Unit = {},
    onChange: ((CalculatorSettings) -> CalculatorSettings) -> Unit,
) {
    Column(
        Modifier
            .fillMaxSize()
            .background(Palette.background)
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        OutlinedTextField(
            value = settings.bpm,
            onValueChange = { entered -> onChange { it.copy(bpm = entered) } },
            label = { Text("BPM") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )

        Labelled("signature") {
            for (s in listOf("4/4", "3/4", "6/8", "7/8")) {
                Chip(s, settings.sig == s) { onChange { it.copy(sig = s) } }
            }
        }
        Labelled("BPM unit") {
            for (u in listOf("1/4", "3/8", "1/8")) {
                Chip(u, settings.bpmUnit == u) { onChange { it.copy(bpmUnit = u) } }
            }
        }
        Labelled("rate") {
            for (r in listOf(44_100, 48_000, 96_000)) {
                Chip("$r", settings.sampleRate == r) { onChange { it.copy(sampleRate = r) } }
            }
        }
        Labelled("bars") {
            for (b in listOf(1L, 2L, 4L, 8L, 16L)) {
                Chip("$b", settings.bars == b) { onChange { it.copy(bars = b) } }
            }
        }

        if (problem != null) {
            Text(problem, color = Palette.bad, fontSize = 13.sp)
            return@Column
        }
        if (sums == null) {
            Text("enter a tempo", color = Palette.dim)
            return@Column
        }

        Card(colors = CardDefaults.cardColors(containerColor = Palette.surface)) {
            Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Line("beat", "${fmt(sums.secondsPerBeat * 1000, 4)} ms · ${fmt(sums.samplesPerBeat, 4)} smp", sums.beatSampleExact)
                Line("bar", "${fmt(sums.secondsPerBar, 6)} s · ${fmt(sums.samplesPerBar, 4)} smp", sums.barSampleExact)
                Line("bar rate", "${fmt(sums.barHz, 6)} Hz · ${fmt(sums.barsPerMinute, 4)} bars/min", null)
                Line("beats/bar", fmt(sums.beatsPerBar, 4), null)
                Line(
                    "${sums.bars} bars",
                    "${fmt(sums.totalSeconds, 6)} s · ${sums.totalSamplesRounded} smp",
                    sums.totalSampleExact,
                )
            }
        }

        OutlinedButton(onClick = onSendToCutter, modifier = Modifier.fillMaxWidth()) {
            Text("Send ${fmt(sums.tempo, 3)} BPM to the cutter")
        }

        Text(
            "NOTE VALUES",
            color = Palette.dim,
            fontSize = 11.sp,
            fontWeight = FontWeight.Bold,
            modifier = Modifier.padding(top = 8.dp),
        )
        Text(
            "· marks a value that lands on a whole sample. One that does not " +
                "drifts out of the grid over a long loop.",
            color = Palette.dim,
            fontSize = 11.sp,
        )

        Card(colors = CardDefaults.cardColors(containerColor = Palette.surface)) {
            // The table is wider than a phone. It scrolls sideways inside its
            // own card rather than squeezing the columns until the digits that
            // matter are the ones that got dropped.
            Column(Modifier.horizontalScroll(rememberScrollState()).padding(12.dp)) {
                NoteHeader()
                for (flavour in listOf("straight", "dotted", "triplet")) {
                    val rows = sums.notes.filter { it.flavour == flavour }
                    if (rows.isEmpty()) continue
                    Spacer(Modifier.height(6.dp))
                    Text(flavour, color = Palette.dim, fontSize = 11.sp)
                    for (row in rows) NoteRowView(row)
                }
            }
        }
        Spacer(Modifier.height(24.dp))
    }
}

@Composable
private fun Labelled(label: String, content: @Composable () -> Unit) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text(label, color = Palette.dim, fontSize = 12.sp, modifier = Modifier.width(88.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) { content() }
    }
}

@Composable
private fun Chip(label: String, selected: Boolean, onClick: () -> Unit) {
    FilterChip(selected = selected, onClick = onClick, label = { Text(label, fontSize = 12.sp) })
}

@Composable
private fun Line(label: String, value: String, exact: Boolean?) {
    Row {
        Text(
            label,
            color = Palette.dim,
            fontSize = 12.sp,
            fontFamily = FontFamily.Monospace,
            modifier = Modifier.width(88.dp),
        )
        Text(
            value + if (exact == true) "  ·" else "",
            color = if (exact == false) Palette.warn else Palette.text,
            fontSize = 12.sp,
            fontFamily = FontFamily.Monospace,
        )
    }
}

@Composable
private fun NoteHeader() {
    Row {
        Cell("value", 72.dp, Palette.dim)
        Cell("ms", 96.dp, Palette.dim)
        Cell("Hz", 88.dp, Palette.dim)
        Cell("samples", 112.dp, Palette.dim)
    }
}

@Composable
private fun NoteRowView(row: NoteRow) {
    Row {
        Cell(row.label, 72.dp, Palette.text)
        Cell(fmt(row.ms, 4), 96.dp, Palette.text)
        Cell(fmt(row.hz, 4), 88.dp, Palette.text)
        Cell(
            fmt(row.samples, 3) + if (row.sampleExact) " ·" else "",
            112.dp,
            if (row.sampleExact) Palette.text else Palette.warn,
        )
    }
}

@Composable
private fun Cell(text: String, width: androidx.compose.ui.unit.Dp, colour: Color) {
    Text(
        text,
        color = colour,
        fontSize = 12.sp,
        fontFamily = FontFamily.Monospace,
        modifier = Modifier.width(width),
    )
}

/** Enough digits to be useful, not so many that the column stops lining up. */
private fun fmt(value: Double, decimals: Int): String =
    if (value == value.toLong().toDouble()) {
        value.toLong().toString()
    } else {
        "%.${decimals}f".format(value).trimEnd('0').trimEnd('.')
    }
