package org.loopslcr.app

import kotlin.math.log10
import kotlin.math.pow

/**
 * What the filter does, if anything.
 *
 * The ordinals are the wire: the native side reads them as numbers, so the order
 * of this list is part of the boundary and not a detail of the Kotlin. `Off`
 * being first means an unknown number arriving from anywhere lands on off, which
 * is the one choice that cannot surprise anybody.
 */
enum class FxMode(val label: String) {
    Off("OFF"),
    LowPass("LP"),
    HighPass("HP"),
    BandPass("BP"),
}

/** Which of the two boxes hears the other. Ordinals are the wire, as above. */
enum class FxRoute(val label: String) {
    FilterFirst("FLT ▸ DRV"),
    DriveFirst("DRV ▸ FLT"),
}

/**
 * How long an echo is, said musically.
 *
 * A fraction of a *bar*, not of the loop: a loop can be four bars or thirty-two,
 * and "an eighth" has to mean the same length of time in both. The loop's bar
 * count turns one into the other, and it is the only number needed to do it.
 *
 * Dotted values are here and triplets are not, because a dotted eighth against a
 * straight loop is the sound this list exists for and a triplet delay is a thing
 * you write into the loop rather than hang off it.
 */
enum class DelayDivision(val label: String, val ofBar: Float) {
    Sixteenth("1/16", 1f / 16f),
    Eighth("1/8", 1f / 8f),
    DottedEighth("1/8.", 3f / 16f),
    Quarter("1/4", 1f / 4f),
    DottedQuarter("1/4.", 3f / 8f),
    Half("1/2", 1f / 2f),
    Bar("1 bar", 1f),
}

/**
 * The insert on the sum, as the panel has it.
 *
 * It is not part of [Settings] and that is deliberate: `Settings` describes a
 * cut and can be written into a file's name, where a filter sweep has no
 * meaning. This describes a performance. Nothing here is exported, for the same
 * reason the varispeed and the motion are not — the cut is what the file is, the
 * insert is what your hands were doing to it.
 */
data class Fx(
    val mode: FxMode = FxMode.Off,
    val cutoffHz: Float = 1_000f,
    val resonance: Float = 0f,
    val drive: Float = 0f,
    val output: Float = 1f,
    val route: FxRoute = FxRoute.FilterFirst,
    val delayMix: Float = 0f,
    /** Whether the echo is locked to the grid or free in milliseconds. */
    val delaySynced: Boolean = true,
    val delayDivision: DelayDivision = DelayDivision.Eighth,
    val delayMs: Float = 375f,
    val delayFeedback: Float = 0.4f,
    val delayDamping: Float = 0.3f,
    val pingPong: Boolean = false,
    val freeze: Boolean = false,
    val reverbMix: Float = 0f,
    val reverbSize: Float = 0.6f,
    val reverbDamping: Float = 0.4f,
    val reverbPredelayMs: Float = 20f,
) {
    /**
     * Whether this position of the panel is indistinguishable from a wire.
     *
     * Freeze counts as on at any mix: a held line you cannot hear is still a
     * held line, and letting it go because the mix was down would lose the thing
     * in the moment you reached for the knob.
     */
    val isWire: Boolean
        get() = mode == FxMode.Off &&
            drive <= 0f &&
            output == 1f &&
            delayMix <= 0f &&
            !freeze &&
            reverbMix <= 0f

    /**
     * The echo as a fraction of the loop, or zero when it is not locked to one.
     *
     * The division is per bar and the native side wants it per loop, because the
     * loop is the only length the preview actually knows. Without a bar count
     * there is nothing to divide by, so an unplanned loop is not synced — better
     * than guessing four.
     */
    fun syncFraction(bars: Long): Float =
        if (delaySynced && bars > 0) delayDivision.ofBar / bars.toFloat() else 0f

    /** The free echo in output samples, which is what the delay counts. */
    fun freeSamples(sampleRate: Int): Float =
        (delayMs.coerceIn(1f, 8_000f) / 1000f) * sampleRate.coerceAtLeast(1)
}

/** The longest free echo the line can hold. */
const val FX_MAX_DELAY_MS = 2_000f

/** Where a free echo time sits on its slider, and back. Linear in milliseconds. */
internal fun delayMsFrom(position: Float): Float =
    (position.coerceIn(0f, 1f) * FX_MAX_DELAY_MS).coerceAtLeast(1f)

internal fun delayTravel(ms: Float): Float = (ms / FX_MAX_DELAY_MS).coerceIn(0f, 1f)

/** The longest a room can be made to wait before it answers. */
const val FX_MAX_PREDELAY_MS = 250f

/** A time as a person reads it. */
internal fun millis(ms: Float): String = "%.0f ms".format(ms)

/** The bottom and top of the cutoff sweep. Roughly what a person can hear. */
const val FX_LOW_HZ = 20f
const val FX_HIGH_HZ = 20_000f

/**
 * Where a cutoff sits on its slider, and back.
 *
 * Logarithmic, because pitch is: linear travel would spend nine tenths of the
 * slider above 2 kHz and give the octave the bass lives in about a millimetre.
 * Three decades, so every one of them gets a third of the sweep and a semitone
 * costs the same distance wherever the corner happens to be.
 */
internal fun cutoffTravel(hz: Float): Float =
    (log10(hz.coerceIn(FX_LOW_HZ, FX_HIGH_HZ) / FX_LOW_HZ) / 3f).coerceIn(0f, 1f)

internal fun cutoffFrom(position: Float): Float =
    (FX_LOW_HZ * 10f.pow(3f * position.coerceIn(0f, 1f))).coerceIn(FX_LOW_HZ, FX_HIGH_HZ)

/** The corner as a person reads it: hertz below a thousand, kilohertz above. */
internal fun cutoffLabel(hz: Float): String =
    if (hz < 1_000f) "%.0f Hz".format(hz) else "%.2f kHz".format(hz / 1_000f)

/** A knob that runs nought to one, as a percentage. */
internal fun percent(value: Float): String = "%.0f %%".format(value.coerceIn(0f, 1f) * 100f)
