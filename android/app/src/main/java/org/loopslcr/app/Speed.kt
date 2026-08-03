package org.loopslcr.app

import kotlin.math.pow

/**
 * The playback ratio a speed setting asks for, worked out locally.
 *
 * # Why this exists at all, when Rust already computes it exactly
 *
 * It does, and its answer is the one that gets written to the file: an exact
 * rational, so 90 BPM from 103 is 90/103 and not a float that drifts. But that
 * answer arrives over a debounce and a native call, and the preview was waiting
 * for it — so a finger on the pitch slider heard nothing until it stopped
 * moving. A live control that answers only on release is not a live control.
 *
 * So this is a *deliberately* approximate, deliberately local second opinion,
 * used for one thing: telling the running preview what to glide towards, now.
 * When the plan arrives it overwrites this with the exact value, and the glide
 * makes the correction inaudible — it is a fraction of a cent over 120 ms.
 *
 * **This number must never reach a file.** It exists between a finger moving and
 * the pipeline answering, and nowhere else. Everything written to disk comes
 * from the exact rational arithmetic in the core.
 */
object Speed {

    /** Ratios outside this cannot be played; the preview engine clamps to it. */
    private val PLAYABLE = 0.25..4.0

    /**
     * What [settings] asks for, given the tempo the file is running at.
     *
     * Null when the answer cannot be had honestly: a target tempo with no source
     * tempo to divide by is not a ratio of 1, it is a question — and pushing 1.0
     * at the preview would be a silent lie about pitch.
     */
    fun ratio(settings: Settings, sourceTempo: Double?): Double? {
        val raw = when (settings.speedMode) {
            SpeedMode.Semitones -> 2.0.pow(settings.semitones / 12.0)
            SpeedMode.TargetBpm -> {
                val target = settings.targetBpm ?: return null
                val source = sourceTempo ?: return null
                if (source <= 0.0 || target <= 0.0) return null
                target / source
            }
        }
        if (!raw.isFinite() || raw <= 0.0) return null
        return raw.coerceIn(PLAYABLE.start, PLAYABLE.endInclusive)
    }
}
