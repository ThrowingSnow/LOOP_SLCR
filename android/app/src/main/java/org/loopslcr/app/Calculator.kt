package org.loopslcr.app

import org.json.JSONObject
import org.loopslcr.Native

/** One row of the note-value table. */
data class NoteRow(
    val label: String,
    val flavour: String,
    val ms: Double,
    val hz: Double,
    val samples: Double,
    val sampleExact: Boolean,
    val perBar: Double,
)

/** Everything that follows from a tempo, a signature, a unit and a rate. */
data class Sums(
    val tempo: Double,
    val sig: String,
    val bpmUnit: String,
    val sampleRate: Int,
    val bars: Long,
    val beatsPerBar: Double,
    val barsPerMinute: Double,
    val secondsPerBeat: Double,
    val samplesPerBeat: Double,
    val beatSampleExact: Boolean,
    val secondsPerBar: Double,
    val samplesPerBar: Double,
    val barSampleExact: Boolean,
    val barHz: Double,
    val totalSeconds: Double,
    val totalSamplesRounded: Long,
    val totalSampleExact: Boolean,
    val notes: List<NoteRow>,
)

/** What the calculator screen is asking about. */
data class CalculatorSettings(
    val bpm: String = "120",
    val sig: String = "4/4",
    val bpmUnit: String = "1/4",
    val sampleRate: Int = 48_000,
    val bars: Long = 4,
) {
    fun toJson(): String {
        val o = JSONObject()
        bpm.toDoubleOrNull()?.let { o.put("bpm", it) }
        o.put("sig", sig)
        o.put("bpmUnit", bpmUnit)
        o.put("sampleRate", sampleRate)
        o.put("bars", bars)
        return o.toString()
    }
}

/**
 * The calculator, computed natively.
 *
 * Not a line of this arithmetic is done in Kotlin, and that is the point. The
 * cutter's bar length and this screen's bar length are the same expression over
 * the same exact rationals; a reimplementation here would agree for a while and
 * then, at some tempo nobody tested, quietly not.
 */
object Calculator {
    fun compute(settings: CalculatorSettings): Result<Sums> = runCatching {
        val o = JSONObject(Native.calculate(settings.toJson()))
        val rows = o.getJSONArray("notes")
        Sums(
            tempo = o.getDouble("tempo"),
            sig = o.getString("sig"),
            bpmUnit = o.getString("bpmUnit"),
            sampleRate = o.getInt("sampleRate"),
            bars = o.getLong("bars"),
            beatsPerBar = o.getDouble("beatsPerBar"),
            barsPerMinute = o.getDouble("barsPerMinute"),
            secondsPerBeat = o.getDouble("secondsPerBeat"),
            samplesPerBeat = o.getDouble("samplesPerBeat"),
            beatSampleExact = o.getBoolean("beatSampleExact"),
            secondsPerBar = o.getDouble("secondsPerBar"),
            samplesPerBar = o.getDouble("samplesPerBar"),
            barSampleExact = o.getBoolean("barSampleExact"),
            barHz = o.getDouble("barHz"),
            totalSeconds = o.getDouble("totalSeconds"),
            totalSamplesRounded = o.getLong("totalSamplesRounded"),
            totalSampleExact = o.getBoolean("totalSampleExact"),
            notes = (0 until rows.length()).map { i ->
                val r = rows.getJSONObject(i)
                NoteRow(
                    label = r.getString("label"),
                    flavour = r.getString("flavour"),
                    ms = r.getDouble("ms"),
                    hz = r.getDouble("hz"),
                    samples = r.getDouble("samples"),
                    sampleExact = r.getBoolean("sampleExact"),
                    perBar = r.getDouble("perBar"),
                )
            },
        )
    }
}
