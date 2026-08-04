package org.loopslcr.app

import android.content.Context
import android.content.pm.ActivityInfo

/** How the mixer is drawn: a desk of vertical strips, or a list of rows. */
enum class MixerView { Desk, Rows }

/**
 * Which way up the app is held.
 *
 * `Auto` is the default and means the device decides — the honest one, since
 * the phone already knows which way it is being held. The other two exist
 * because a phone lying on a desk beside a controller has an opinion the
 * accelerometer does not share.
 */
enum class Screen {
    Auto,
    Portrait,
    Landscape,
    ;

    /** What to hand `Activity.setRequestedOrientation`. */
    fun requested(): Int = when (this) {
        Auto -> ActivityInfo.SCREEN_ORIENTATION_UNSPECIFIED
        Portrait -> ActivityInfo.SCREEN_ORIENTATION_PORTRAIT
        Landscape -> ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE
    }
}

/**
 * The choices about looks, as opposed to about audio.
 *
 * Kept apart from [Settings] on purpose: those describe a cut and belong to a
 * file, these describe a screen and belong to a person. Mixing them would mean
 * a saved cut carrying somebody's fader orientation around with it.
 */
data class Display(
    val mixer: MixerView = MixerView.Desk,
    val screen: Screen = Screen.Auto,
)

/**
 * The display choices, remembered between runs.
 *
 * `SharedPreferences` rather than anything larger: two enums, read once at
 * start-up and written when a chip is tapped. A setting that forgot itself on
 * every launch would be worse than no setting — you would have to make the
 * choice again every time, which is the opposite of what a preference is for.
 *
 * Reading an unknown value falls back to the default rather than throwing: the
 * file survives an app downgrade, and a name that no longer exists is not worth
 * a crash on launch.
 */
class Preferences(context: Context) {
    private val store = context.getSharedPreferences("display", Context.MODE_PRIVATE)

    fun load(): Display = Display(
        mixer = read(MIXER, MixerView.entries, MixerView.Desk),
        screen = read(SCREEN, Screen.entries, Screen.Auto),
    )

    fun save(display: Display) {
        store.edit()
            .putString(MIXER, display.mixer.name)
            .putString(SCREEN, display.screen.name)
            .apply()
    }

    private fun <T : Enum<T>> read(key: String, all: List<T>, fallback: T): T {
        val name = store.getString(key, null) ?: return fallback
        return all.firstOrNull { it.name == name } ?: fallback
    }

    private companion object {
        const val MIXER = "mixer"
        const val SCREEN = "screen"
    }
}
