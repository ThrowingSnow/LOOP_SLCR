package org.loopslcr.app

import org.json.JSONObject

/**
 * What the file is, before anything has been decided about it.
 *
 * Read straight off `analyze`. Nulls are kept as nulls: a tempo that could not
 * be found is not 120, and a UI that fills one in has told the user something
 * untrue about their file.
 */
data class Analysis(
    val channels: Int,
    val sampleRate: Int,
    val bitsPerSample: Int,
    val frames: Long,
    val durationSeconds: Double,
    val peak: Double,
    val tailFrames: Long,
    val declaredTempo: Double?,
    val nameTempo: Double?,
    val nameBars: Long?,
    val tempo: Double?,
    val loopBars: Long?,
    val workflow: String,
) {
    companion object {
        fun from(o: JSONObject) = Analysis(
            channels = o.getInt("channels"),
            sampleRate = o.getInt("sampleRate"),
            bitsPerSample = o.getInt("bitsPerSample"),
            frames = o.getLong("frames"),
            durationSeconds = o.getDouble("durationSeconds"),
            peak = o.getDouble("peak"),
            tailFrames = o.getLong("tailFrames"),
            declaredTempo = o.optDoubleOrNull("declaredTempo"),
            nameTempo = o.optDoubleOrNull("nameTempo"),
            nameBars = o.optLongOrNull("nameBars"),
            tempo = o.optDoubleOrNull("tempo"),
            loopBars = o.optLongOrNull("loopBars"),
            workflow = o.optString("workflow", "unclear"),
        )
    }
}

/** What a cut would do — the dry run, in full. */
data class Plan(
    val tempo: Double,
    val tempoSource: String,
    val bars: Long,
    val barsSource: String,
    val workflowDetected: String,
    val workflowChosen: String,
    val skipBars: Long,
    val regionStart: Long,
    val regionEnd: Long,
    val loopFrames: Long,
    val shortBy: Long,
    val fadeFrames: Long,
    val ratio: Double,
    val ratioExact: Boolean,
    val semitones: Double,
    val resultingTempo: Double,
    val outputFrames: Long,
    val peak: Double,
    val clips: Boolean,
    val tape: Boolean,
    val normalizeGain: Double?,
    val dithered: Boolean,
) {
    companion object {
        fun from(o: JSONObject) = Plan(
            tempo = o.getDouble("tempo"),
            tempoSource = o.getString("tempoSource"),
            bars = o.getLong("bars"),
            barsSource = o.getString("barsSource"),
            workflowDetected = o.getString("workflowDetected"),
            workflowChosen = o.getString("workflowChosen"),
            skipBars = o.getLong("skipBars"),
            regionStart = o.getLong("regionStart"),
            regionEnd = o.getLong("regionEnd"),
            loopFrames = o.getLong("loopFrames"),
            shortBy = o.getLong("shortBy"),
            fadeFrames = o.getLong("fadeFrames"),
            ratio = o.getDouble("ratio"),
            ratioExact = o.getBoolean("ratioExact"),
            semitones = o.getDouble("semitones"),
            resultingTempo = o.getDouble("resultingTempo"),
            outputFrames = o.getLong("outputFrames"),
            peak = o.getDouble("peak"),
            clips = o.getBoolean("clips"),
            tape = o.getBoolean("tape"),
            normalizeGain = o.optDoubleOrNull("normalizeGain"),
            dithered = o.getBoolean("dithered"),
        )
    }
}

/** How the varispeed is being asked for. The two are mutually exclusive. */
enum class SpeedMode { Semitones, TargetBpm }

/**
 * Everything the user can set, in the app's own terms.
 *
 * Turned into the parameter JSON by [toJson] and nowhere else. A field left at
 * its default is *left out* rather than sent: the native side already knows what
 * the default means, and sending it would make this class a second place where
 * defaults are written down.
 */
data class Settings(
    val speedMode: SpeedMode = SpeedMode.Semitones,
    val semitones: Double = 0.0,
    val targetBpm: Double? = null,
    val bars: Long? = null,
    val skip: Long? = null,
    val workflow: String = "auto",
    val depth: String = "24",
    val normalize: Boolean = false,
    val tape: Boolean = false,
    val wow: Double = 0.3,
    val flutter: Double = 0.15,
    val allowShort: Boolean = false,
    val snap: Boolean = false,
) {
    fun toJson(): String {
        val o = JSONObject()
        when (speedMode) {
            SpeedMode.Semitones -> if (semitones != 0.0) o.put("semitones", semitones)
            SpeedMode.TargetBpm -> targetBpm?.let { o.put("targetBpm", it) }
        }
        bars?.let { o.put("bars", it) }
        skip?.let { o.put("skip", it) }
        if (workflow != "auto") o.put("workflow", workflow)
        if (depth != "24") o.put("depth", depth)
        if (normalize) o.put("normalize", true)
        if (allowShort) o.put("allowShort", true)
        if (snap) o.put("snap", true)
        if (tape) {
            o.put("tape", true)
            o.put("wow", wow)
            o.put("flutter", flutter)
        }
        return o.toString()
    }
}

/** `optDouble` returns NaN for a missing key; a null has to stay a null. */
private fun JSONObject.optDoubleOrNull(key: String): Double? =
    if (isNull(key)) null else getDouble(key)

private fun JSONObject.optLongOrNull(key: String): Long? =
    if (isNull(key)) null else getLong(key)
