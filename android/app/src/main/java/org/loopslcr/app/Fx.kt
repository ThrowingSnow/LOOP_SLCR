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
) {
    /** Whether this position of the panel is indistinguishable from a wire. */
    val isWire: Boolean
        get() = mode == FxMode.Off && drive <= 0f && output == 1f
}

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
