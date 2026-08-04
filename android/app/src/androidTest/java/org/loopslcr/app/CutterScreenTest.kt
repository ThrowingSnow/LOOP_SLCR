package org.loopslcr.app

import android.graphics.Bitmap
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.ui.unit.dp
import androidx.compose.ui.Modifier
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.onRoot
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertFalse
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import java.io.File
import kotlin.math.absoluteValue

/**
 * The screen with a file in it.
 *
 * The engine tests prove the audio; this proves the layout that shows it —
 * including the waveform canvas, which draws from a `FloatArray` and would fail
 * on a length mismatch that no type would have caught.
 *
 * It also leaves a screenshot behind. Not an assertion: a way to look at what
 * the thing renders without an emulator on screen.
 */
class CutterScreenTest {
    @get:Rule
    val compose = createComposeRule()

    private val name = "103 loop.wav"

    @Test
    fun a_loaded_file_renders_with_its_waveform_and_its_numbers() {
        val raw = EngineTest.wav(bars = 8)
        val analysis = Engine.analyze(org.loopslcr.Native.direct(raw), "200 loop.wav")
        val peaks = Engine.peaks(org.loopslcr.Native.direct(raw), 512)
        // A partial cut, so the screenshot shows a region rather than the whole file.
        val plan = Engine.plan(org.loopslcr.Native.direct(raw), "200 loop.wav", Settings(bars = 4L, skip = 2L))

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                CutterScreen(
                    loaded = Loaded(name, org.loopslcr.Native.direct(raw), analysis, peaks),
                    settings = Settings(tape = true, bars = 4L, skip = 2L),
                    plan = plan,
                    busy = Busy.Idle,
                    problem = null,
                    onOpen = {},
                    onExport = {},
                    onChange = {},
                    onDismissProblem = {},
                )
            }
        }

        compose.onNodeWithText(name).assertExists()
        compose.onNodeWithText("Export").assertExists()

        // Groups start folded, so the tape panel is not composed until its
        // header is tapped — and then it is there. The tape sliders exist only
        // when the character is on: a panel that renders whether or not it
        // applies is a panel that lies.
        compose.onNodeWithText("Tape character").assertDoesNotExist()
        compose.onNodeWithText("TAPE").performClick()
        compose.waitForIdle()
        compose.onNodeWithText("Tape character").assertExists()

        val shot = compose.onRoot().captureToImage()
        assertTrue(shot.width > 0 && shot.height > 0)
        save(shot.asAndroidBitmap(), "cutter.png")
    }

    @Test
    fun the_file_figures_hide_behind_the_name_until_it_is_tapped() {
        // They used to sit between the waveform and every control, so the first
        // thing you could actually change was a scroll away. Behind the name
        // they are one tap off — but the tap has to work, and the figures must
        // not be on screen before it.
        val raw = EngineTest.wav(bars = 8)
        val analysis = Engine.analyze(org.loopslcr.Native.direct(raw), "200 loop.wav")
        val peaks = Engine.peaks(org.loopslcr.Native.direct(raw), 512)

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                CutterScreen(
                    loaded = Loaded(name, org.loopslcr.Native.direct(raw), analysis, peaks),
                    settings = Settings(),
                    plan = null,
                    busy = Busy.Idle,
                    problem = null,
                    onOpen = {},
                    onExport = {},
                    onChange = {},
                    onDismissProblem = {},
                )
            }
        }

        compose.onNodeWithText("format").assertDoesNotExist()
        compose.onNodeWithText(name).performClick()
        compose.waitForIdle()
        compose.onNodeWithText("format").assertExists()
        compose.onNodeWithText("peak").assertExists()

        // And it closes again, or it is a one-way door rather than a drawer.
        compose.onNodeWithText(name).performClick()
        compose.waitForIdle()
        compose.onNodeWithText("format").assertDoesNotExist()
    }

    @Test
    fun play_holds_the_corner_and_open_waits_behind_the_name() {
        // The corner belongs to whatever is pressed most. A file is chosen once
        // and then listened to for minutes, so Play took it — and Open, which
        // throws away every setting on the screen, moved somewhere deliberate.
        val raw = EngineTest.wav(bars = 8)
        val analysis = Engine.analyze(org.loopslcr.Native.direct(raw), "200 loop.wav")
        val peaks = Engine.peaks(org.loopslcr.Native.direct(raw), 512)

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                CutterScreen(
                    loaded = Loaded(name, org.loopslcr.Native.direct(raw), analysis, peaks),
                    settings = Settings(),
                    plan = null,
                    busy = Busy.Idle,
                    problem = null,
                    onOpen = {},
                    onExport = {},
                    onChange = {},
                    onDismissProblem = {},
                )
            }
        }

        compose.onNodeWithText("Play").assertExists()
        compose.onNodeWithText("Open another file").assertDoesNotExist()

        compose.onNodeWithText(name).performClick()
        compose.waitForIdle()
        compose.onNodeWithText("Open another file").assertExists()
    }

    @Test
    fun a_folded_plan_card_still_says_that_the_cut_clips() {
        // The rule that makes folding safe at all: detail may hide, trouble may
        // not. A card that could swallow "clips" would be worse than one that
        // does not fold, because the number it hid was the reason to look.
        val raw = EngineTest.wav(bars = 8)
        val analysis = Engine.analyze(org.loopslcr.Native.direct(raw), "200 loop.wav")
        val peaks = Engine.peaks(org.loopslcr.Native.direct(raw), 512)
        val plan = Engine.plan(org.loopslcr.Native.direct(raw), "200 loop.wav", Settings())
            .copy(clips = true, peak = 1.4998)

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                CutterScreen(
                    loaded = Loaded(name, org.loopslcr.Native.direct(raw), analysis, peaks),
                    settings = Settings(),
                    plan = plan,
                    busy = Busy.Idle,
                    problem = null,
                    onOpen = {},
                    onExport = {},
                    onChange = {},
                    onDismissProblem = {},
                )
            }
        }

        // Everything starts folded now, so this is the state the user meets.
        compose.onNodeWithText("cut").assertDoesNotExist()
        compose.onNodeWithText("clips", substring = true).assertExists()

        // And unfolding still gives the detail back.
        compose.onNodeWithText("clips", substring = true).performClick()
        compose.waitForIdle()
        compose.onNodeWithText("cut").assertExists()
    }

    @Test
    fun the_pitch_slider_sits_under_the_waveform_not_in_a_folded_group() {
        // It is the one control held *while listening*, so it has to be within
        // sight of the picture it acts on. Checked by geometry rather than by
        // reading the source: "under the waveform" is a claim about pixels.
        val raw = EngineTest.wav(bars = 8)
        val analysis = Engine.analyze(org.loopslcr.Native.direct(raw), "200 loop.wav")
        val peaks = Engine.peaks(org.loopslcr.Native.direct(raw), 512)
        val plan = Engine.plan(org.loopslcr.Native.direct(raw), "200 loop.wav", Settings())

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                CutterScreen(
                    loaded = Loaded(name, org.loopslcr.Native.direct(raw), analysis, peaks),
                    settings = Settings(semitones = -3.0),
                    plan = plan,
                    busy = Busy.Idle,
                    problem = null,
                    onOpen = {},
                    onExport = {},
                    onChange = {},
                    onDismissProblem = {},
                )
            }
        }

        // The claim is "reachable while watching the waveform", so the check is
        // that it is in the viewport *without scrolling* — together with the
        // line that sits directly under the waveform. Comparing raw bounds
        // against the VARISPEED header does not work: a node scrolled out of
        // view reports zero, which would pass for the wrong reason.
        compose.onNodeWithText("semitones").assertIsDisplayed()
        compose.onNodeWithText("drag a marker", substring = true).assertIsDisplayed()

        val pitch = compose.onNodeWithText("semitones").getBoundsInRoot()
        val hint = compose.onNodeWithText("drag a marker", substring = true).getBoundsInRoot()
        assertTrue("pitch at ${pitch.top}, hint at ${hint.top}", pitch.top > hint.top)

        compose.onNodeWithText("-3.00 st", substring = true).assertExists()
    }

    @Test
    fun a_tempo_arriving_from_elsewhere_reaches_the_field() {
        // The bug behind "send the BPM from the calculator and it just adapts".
        // The field kept its text in a `remember` keyed on the *mode*, so a
        // tempo that arrived while target-BPM mode was already selected left the
        // old number sitting there — which looks exactly like nothing happened.
        val raw = EngineTest.wav(bars = 8)
        val analysis = Engine.analyze(org.loopslcr.Native.direct(raw), "200 loop.wav")
        val peaks = Engine.peaks(org.loopslcr.Native.direct(raw), 512)
        val plan = Engine.plan(org.loopslcr.Native.direct(raw), "200 loop.wav", Settings())

        // Already in target-BPM mode, as it would be on a second send.
        var settings by mutableStateOf(
            Settings(speedMode = SpeedMode.TargetBpm, targetBpm = 150.0),
        )

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                CutterScreen(
                    loaded = Loaded(name, org.loopslcr.Native.direct(raw), analysis, peaks),
                    settings = settings,
                    plan = plan,
                    busy = Busy.Idle,
                    problem = null,
                    onOpen = {},
                    onExport = {},
                    onChange = { change -> settings = change(settings) },
                    onDismissProblem = {},
                )
            }
        }

        compose.onNodeWithText("150").assertExists()

        // What `sendTempoToCutter` does, from outside the screen.
        settings = settings.copy(speedMode = SpeedMode.TargetBpm, targetBpm = 90.0)
        compose.waitForIdle()

        compose.onNodeWithText("90").assertExists()
        compose.onNodeWithText("150").assertDoesNotExist()
    }

    @Test
    fun both_units_sit_behind_their_own_buttons_on_one_row() {
        // It was a button row and a readout row: two lines of a screen whose
        // whole layout exists to keep the waveform in sight, with the number you
        // type a slider away from the number it makes. Now each mode's button
        // carries its own value immediately behind it, and the two share a line.
        //
        // The earlier rule — the field is a quarter of the strip — is gone with
        // that layout. It measured a field that had a row to itself; this one
        // shares the row with two buttons and the other unit's readout.
        val raw = EngineTest.wav(bars = 8)
        val analysis = Engine.analyze(org.loopslcr.Native.direct(raw), "200 loop.wav")
        val peaks = Engine.peaks(org.loopslcr.Native.direct(raw), 512)
        val plan = Engine.plan(org.loopslcr.Native.direct(raw), "200 loop.wav", Settings())

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                CutterScreen(
                    loaded = Loaded(name, org.loopslcr.Native.direct(raw), analysis, peaks),
                    settings = Settings(speedMode = SpeedMode.TargetBpm, targetBpm = 150.0),
                    plan = plan,
                    busy = Busy.Idle,
                    problem = null,
                    onOpen = {},
                    onExport = {},
                    onChange = {},
                    onDismissProblem = {},
                )
            }
        }

        val semitones = compose.onNodeWithText("semitones").getBoundsInRoot()
        val target = compose.onNodeWithText("target BPM").getBoundsInRoot()
        val field = compose.onNodeWithTag("tempoField").getBoundsInRoot()
        val readout = compose.onNodeWithText("st", substring = true).getBoundsInRoot()

        // One row: all four overlap vertically.
        for ((what, bounds) in listOf("target" to target, "field" to field, "st" to readout)) {
            assertTrue(
                "$what at ${bounds.top}..${bounds.bottom}, semitones at " +
                    "${semitones.top}..${semitones.bottom}",
                bounds.top < semitones.bottom && semitones.top < bounds.bottom,
            )
        }

        // And each value is behind its own button, not the other one's.
        assertTrue(
            "the semitone readout at ${readout.left} is not behind its button " +
                "at ${semitones.right}",
            readout.left >= semitones.right,
        )
        assertTrue(
            "the tempo field at ${field.left} is not behind its button at ${target.right}",
            field.left >= target.right,
        )
        // target BPM is pushed to the right-hand end, which is what makes the
        // row read as two pairs rather than as a queue.
        assertTrue(
            "target BPM starts at ${target.left}, semitones ends at ${semitones.right}",
            (target.left - semitones.right).value > 40f,
        )
    }

    @Test
    fun the_mixer_can_be_rows_instead_and_the_numbers_do_not_change() {
        // A desk needs width and a phone held upright has little of it. Which
        // matters more is not something the code can know, so it is a setting —
        // but both views have to be the same instrument underneath.
        val raw = EngineTest.wav(bars = 8)
        val analysis = Engine.analyze(org.loopslcr.Native.direct(raw), "200 loop.wav")
        val peaks = Engine.peaks(org.loopslcr.Native.direct(raw), 512)
        val file = Loaded(name, org.loopslcr.Native.direct(raw), analysis, peaks)

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                MixerScreen(
                    first = file,
                    second = file,
                    gains = 1f to 1f,
                    masterGain = 1f,
                    levels = { Triple(0f, 0f, 0f) },
                    playing = false,
                    onGains = { _, _ -> },
                    onMasterGain = {},
                    view = MixerView.Rows,
                )
            }
        }

        val one = compose.onNodeWithTag("fader1").getBoundsInRoot()
        val two = compose.onNodeWithTag("fader2").getBoundsInRoot()
        val mst = compose.onNodeWithTag("faderMST").getBoundsInRoot()
        assertTrue(
            "rows at ${one.top}, ${two.top}, ${mst.top}",
            one.top < two.top && two.top < mst.top,
        )
        assertTrue("a row is wider than it is tall", one.right - one.left > one.bottom - one.top)
        // All three still say 0.0 dB, from the same arithmetic as the desk —
        // and so does the insert's output trim, which is a fourth reading of the
        // same number because it is the same fader arithmetic underneath.
        compose.onAllNodesWithText("+0.0 dB").assertCountEquals(4)

        save(compose.onRoot().captureToImage().asAndroidBitmap(), "mixer-rows.png")
    }

    @Test
    fun the_display_choices_are_remembered_and_reported() {
        // A setting that forgot itself on every launch would be worse than no
        // setting: you would make the choice again every time, which is the
        // opposite of what a preference is for.
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val preferences = Preferences(context)
        val before = preferences.load()
        try {
            preferences.save(Display(MixerView.Rows, Screen.Landscape))
            assertEquals(Display(MixerView.Rows, Screen.Landscape), Preferences(context).load())
            preferences.save(Display())
            assertEquals(Display(MixerView.Desk, Screen.Auto), Preferences(context).load())
        } finally {
            preferences.save(before)
        }

        var chosen: Display? = null
        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                SettingsScreen(
                    engineVersion = "test",
                    build = "test",
                    display = Display(),
                    onDisplay = { chosen = it },
                )
            }
        }
        compose.onNodeWithTag("mixerRows").performScrollTo().performClick()
        assertEquals(MixerView.Rows, chosen?.mixer)
        compose.onNodeWithTag("screenLandscape").performScrollTo().performClick()
        assertEquals(Screen.Landscape, chosen?.screen)
    }

    @Test
    fun sideways_the_panels_stand_beside_the_waveform_rather_than_under_it() {
        // The same split turned ninety degrees. Measured off the layout's own
        // constraints rather than asked of the device, because a tablet held
        // upright with room to spare wants the wide arrangement too.
        val raw = EngineTest.wav(bars = 8)
        val analysis = Engine.analyze(org.loopslcr.Native.direct(raw), "200 loop.wav")
        val peaks = Engine.peaks(org.loopslcr.Native.direct(raw), 512)
        val file = Loaded(name, org.loopslcr.Native.direct(raw), analysis, peaks)
        var size by mutableStateOf(400.dp to 800.dp)

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                Box(Modifier.size(size.first, size.second)) {
                    CutterScreen(
                        loaded = file,
                        settings = Settings(),
                        plan = null,
                        busy = Busy.Idle,
                        problem = null,
                        onOpen = {},
                        onExport = {},
                        onChange = {},
                        onDismissProblem = {},
                    )
                }
            }
        }

        val tallWave = compose.onNodeWithTag("wave").getBoundsInRoot()
        val tallPanels = compose.onNodeWithTag("panels").getBoundsInRoot()
        assertTrue(
            "upright, the panels are at ${tallPanels.top} and the wave ends at ${tallWave.bottom}",
            tallPanels.top >= tallWave.bottom,
        )

        compose.runOnIdle { size = 800.dp to 400.dp }

        val wideWave = compose.onNodeWithTag("wave").getBoundsInRoot()
        val widePanels = compose.onNodeWithTag("panels").getBoundsInRoot()
        assertTrue(
            "sideways, the panels start at ${widePanels.left} and the wave ends at ${wideWave.right}",
            widePanels.left >= wideWave.right,
        )
        assertTrue(
            "sideways, the picture no longer reaches the panels' row",
            widePanels.top < wideWave.bottom,
        )

        save(compose.onRoot().captureToImage().asAndroidBitmap(), "cutter-wide.png")
    }

    @Test
    fun the_mixer_is_a_desk_with_two_channels_and_a_master() {
        // Read across, not down: three strips side by side let one glance
        // compare two levels. Stacked rows of horizontal sliders make that a
        // scroll and a memory test.
        val raw = EngineTest.wav(bars = 8)
        val analysis = Engine.analyze(org.loopslcr.Native.direct(raw), "200 loop.wav")
        val peaks = Engine.peaks(org.loopslcr.Native.direct(raw), 512)
        val file = Loaded(name, org.loopslcr.Native.direct(raw), analysis, peaks)
        var channels: Pair<Float, Float>? = null
        var master: Float? = null

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                MixerScreen(
                    first = file,
                    second = file,
                    gains = 1f to 1f,
                    masterGain = 1f,
                    levels = { Triple(0f, 0f, 0f) },
                    playing = false,
                    onGains = { a, b -> channels = a to b },
                    onMasterGain = { g -> master = g },
                )
            }
        }

        val one = compose.onNodeWithTag("fader1").getBoundsInRoot()
        val two = compose.onNodeWithTag("fader2").getBoundsInRoot()
        val mst = compose.onNodeWithTag("faderMST").getBoundsInRoot()
        assertTrue(
            "strips at ${one.left}, ${two.left}, ${mst.left}",
            one.left < two.left && two.left < mst.left,
        )
        assertTrue("channel 1 and the master are on different rows", one.top == mst.top)
        // Taller than wide, or it is not a fader.
        val tall = one.bottom - one.top
        val wide = one.right - one.left
        assertTrue("fader is $wide by $tall", tall > wide * 2)
        // And a meter of its own height standing beside each one.
        val meter = compose.onNodeWithTag("meter1").getBoundsInRoot()
        assertTrue(
            "meter ${meter.bottom - meter.top} against fader $tall",
            meter.bottom - meter.top == tall,
        )
        assertTrue("meter at ${meter.left}, fader at ${one.left}", meter.left < one.left)

        // Tapped in the middle of its travel, a fader lands at a quarter of the
        // top — the cube law, felt rather than computed.
        compose.onNodeWithTag("fader1").performClick()
        assertEquals(0.25f, channels?.first ?: -1f, 0.03f)
        assertEquals("the second channel moved too", 1f, channels?.second ?: -1f, 0.0001f)
        assertEquals(null, master)

        compose.onNodeWithTag("faderMST").performClick()
        assertEquals(0.25f, master ?: -1f, 0.03f)

        save(compose.onRoot().captureToImage().asAndroidBitmap(), "mixer.png")
    }

    @Test
    fun the_insert_is_on_the_desk_and_says_whether_it_is_in_the_path() {
        // An insert belongs beside the meter that reads it. On its own tab it
        // would be a control you turn while looking at a picture of a different
        // signal.
        val raw = EngineTest.wav(bars = 8)
        val analysis = Engine.analyze(org.loopslcr.Native.direct(raw), "200 loop.wav")
        val peaks = Engine.peaks(org.loopslcr.Native.direct(raw), 512)
        val file = Loaded(name, org.loopslcr.Native.direct(raw), analysis, peaks)
        var fx by mutableStateOf(Fx())

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                MixerScreen(
                    first = file,
                    second = null,
                    gains = 1f to 1f,
                    masterGain = 1f,
                    levels = { Triple(0f, 0f, 0f) },
                    playing = false,
                    onGains = { _, _ -> },
                    onMasterGain = { },
                    fx = fx,
                    onFx = { fx = it },
                )
            }
        }

        // Untouched, it says so — and says it because it is true, not because
        // the mode happens to read "OFF".
        compose.onNodeWithTag("fxState").assertTextEquals("bypassed")

        compose.onNodeWithTag("fxModeHighPass").performClick()
        compose.runOnIdle {
            assertEquals(FxMode.HighPass, fx.mode)
            assertFalse("a highpass in the path still called itself a wire", fx.isWire)
        }
        compose.onNodeWithTag("fxState").assertTextEquals("in the signal path")

        // The order is a switch with two positions, and picking one is picking
        // the other off.
        compose.onNodeWithTag("fxRouteDriveFirst").performClick()
        compose.runOnIdle { assertEquals(FxRoute.DriveFirst, fx.route) }
        compose.onNodeWithTag("fxRouteFilterFirst").performClick()
        compose.runOnIdle { assertEquals(FxRoute.FilterFirst, fx.route) }

        // And it is above the notes about what is still to come, not instead of
        // them: delay and reverb are still owed.
        compose.onNodeWithTag("fxCutoff").assertExists()
        compose.onNodeWithTag("fxDrive").assertExists()
        compose.onNodeWithTag("fxOutput").assertExists()
    }

    @Test
    fun the_echo_is_asked_for_in_note_values_and_delivered_as_a_fraction() {
        // A division is per *bar*; the preview counts a fraction of the *loop*.
        // Getting that conversion backwards would put an eighth-note delay eight
        // times too long on a thirty-two-bar loop and nobody would notice until
        // they played one.
        val eighth = Fx(delayMix = 0.5f, delayDivision = DelayDivision.Eighth)
        assertEquals(1f / 8f / 4f, eighth.syncFraction(4), 1e-6f)
        assertEquals(1f / 8f / 32f, eighth.syncFraction(32), 1e-6f)
        // A dotted eighth is three sixteenths, which is the whole reason it is
        // on the list.
        assertEquals(
            3f * eighth.copy(delayDivision = DelayDivision.Sixteenth).syncFraction(4),
            eighth.copy(delayDivision = DelayDivision.DottedEighth).syncFraction(4),
            1e-6f,
        )
        // A whole bar of a four-bar loop is a quarter of it.
        assertEquals(0.25f, eighth.copy(delayDivision = DelayDivision.Bar).syncFraction(4), 1e-6f)

        // Free is free: no fraction at all, and the milliseconds become samples.
        val free = eighth.copy(delaySynced = false, delayMs = 500f)
        assertEquals(0f, free.syncFraction(4), 1e-6f)
        assertEquals(24_000f, free.freeSamples(48_000), 1f)
        // And with no plan there is no bar count to divide by, so nothing is
        // claimed rather than four being guessed.
        assertEquals(0f, eighth.syncFraction(0), 1e-6f)
    }

    @Test
    fun freeze_is_on_even_at_no_mix_and_the_panel_says_so() {
        // A held line you cannot hear is still a held line. If the bypass check
        // let it go because the mix was down, the thing would be lost in the
        // moment you reached for the knob to hear it.
        assertTrue(Fx().isWire)
        assertFalse("a freeze at no mix read as bypassed", Fx(freeze = true).isWire)
        assertFalse(Fx(delayMix = 0.3f).isWire)
        assertFalse(Fx(reverbMix = 0.3f).isWire)
    }

    @Test
    fun the_whole_insert_is_reachable_and_the_time_control_swaps_over() {
        val raw = EngineTest.wav(bars = 8)
        val analysis = Engine.analyze(org.loopslcr.Native.direct(raw), "200 loop.wav")
        val peaks = Engine.peaks(org.loopslcr.Native.direct(raw), 512)
        val file = Loaded(name, org.loopslcr.Native.direct(raw), analysis, peaks)
        var fx by mutableStateOf(Fx())

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                MixerScreen(
                    first = file,
                    second = null,
                    gains = 1f to 1f,
                    masterGain = 1f,
                    levels = { Triple(0f, 0f, 0f) },
                    playing = false,
                    onGains = { _, _ -> },
                    onMasterGain = { },
                    fx = fx,
                    onFx = { fx = it },
                )
            }
        }

        // Synced by default, so the note values are what is offered.
        compose.onNodeWithTag("fxDivQuarter").performScrollTo().performClick()
        compose.runOnIdle { assertEquals(DelayDivision.Quarter, fx.delayDivision) }

        // FREE puts a millisecond knob where the note values were, rather than
        // showing both and leaving which one is in charge to be guessed.
        compose.onNodeWithTag("fxDelayFree").performScrollTo().performClick()
        compose.runOnIdle { assertFalse("SYNC stayed on", fx.delaySynced) }
        compose.onNodeWithTag("fxDelayMs").performScrollTo().assertIsDisplayed()
        compose.onAllNodesWithTag("fxDivQuarter").assertCountEquals(0)

        compose.onNodeWithTag("fxDelaySync").performScrollTo().performClick()
        compose.runOnIdle { assertTrue("FREE stayed on", fx.delaySynced) }
        compose.onNodeWithTag("fxDivQuarter").performScrollTo().assertIsDisplayed()

        // Ping-pong and freeze are switches, and the panel notices freeze even
        // with every mix at nothing.
        compose.onNodeWithTag("fxPingPong").performScrollTo().performClick()
        compose.runOnIdle { assertTrue("ping-pong did not latch", fx.pingPong) }
        compose.onNodeWithTag("fxFreeze").performScrollTo().performClick()
        compose.runOnIdle { assertTrue("freeze did not latch", fx.freeze) }
        compose.onNodeWithTag("fxState").assertTextEquals("in the signal path")

        // And the room is all there.
        compose.onNodeWithTag("fxReverbMix").performScrollTo().assertIsDisplayed()
        compose.onNodeWithTag("fxReverbSize").performScrollTo().assertIsDisplayed()
        compose.onNodeWithTag("fxReverbPre").performScrollTo().assertIsDisplayed()
    }

    @Test
    fun the_cutoff_sweep_gives_every_octave_the_same_room() {
        // Linear travel would spend nine tenths of the slider above 2 kHz and
        // leave the octave the bass lives in about a millimetre. Three decades,
        // a third of the sweep each, so a semitone costs the same distance
        // wherever the corner is.
        assertEquals(0f, cutoffTravel(FX_LOW_HZ), 0.001f)
        assertEquals(1f, cutoffTravel(FX_HIGH_HZ), 0.001f)
        // The middle of the slider is the geometric middle of the range, not
        // the arithmetic one — 632 Hz rather than 10 kHz.
        assertTrue("the middle is ${cutoffFrom(0.5f)} Hz", cutoffFrom(0.5f) in 600f..680f)

        for (hz in listOf(20f, 80f, 440f, 1_000f, 8_000f, 20_000f)) {
            assertEquals(hz, cutoffFrom(cutoffTravel(hz)), hz * 0.001f)
        }
        // An octave costs the same travel down low as up high.
        val low = cutoffTravel(160f) - cutoffTravel(80f)
        val high = cutoffTravel(8_000f) - cutoffTravel(4_000f)
        assertEquals(low, high, 0.001f)

        assertEquals("440 Hz", cutoffLabel(440f))
        assertEquals("4.40 kHz", cutoffLabel(4_400f))
    }

    @Test
    fun the_meter_scale_and_the_fader_travel_are_the_arithmetic_a_hand_expects() {
        // Two decisions worth pinning, because both are easy to get backwards
        // and neither is visible in a screenshot.
        //
        // The meter is decibels: linear, everything quiet enough to be worth
        // adjusting sits in the bottom tenth of the bar and the meter is
        // decoration. Full scale is the top, −48 dB the bottom.
        assertEquals(1f, meterScale(1f), 0.001f)
        assertEquals(0f, meterScale(0f), 0.001f)
        assertTrue("half amplitude is not near the top: ${meterScale(0.5f)}", meterScale(0.5f) in 0.85f..0.9f)
        assertTrue("a quiet signal vanishes: ${meterScale(0.01f)}", meterScale(0.01f) > 0.1f)

        // The fader is the other way round: linear in loudness, so halfway up
        // sounds about half as loud rather than 6 dB down.
        for (position in listOf(0f, 0.25f, 0.5f, 1f)) {
            assertEquals(position, travel(fromTravel(position)), 0.001f)
        }
        assertTrue(
            "halfway is ${decibels(fromTravel(0.5f))}, which is not about −18 dB",
            fromTravel(0.5f) in 0.22f..0.28f,
        )

        // The top is +6 dB and unity is a little below it, where a desk puts
        // it — a fader that could only cut leaves "too quiet" with no control.
        assertEquals(MAX_GAIN, fromTravel(1f), 0.001f)
        assertEquals("+6.0 dB", decibels(fromTravel(1f)))
        assertTrue("unity sits at $UNITY_TRAVEL", UNITY_TRAVEL in 0.75f..0.82f)
        assertEquals(1f, fromTravel(UNITY_TRAVEL), 0.0001f)
        // And it has a detent, so exactly 0 dB is reachable with a finger.
        assertEquals(1f, fromTravel(UNITY_TRAVEL + 0.015f), 0.0001f)
        assertTrue("the detent never ends", fromTravel(UNITY_TRAVEL + 0.1f) > 1.05f)
    }

    @Test
    fun folding_the_lanes_halves_the_picture_and_keeps_the_file() {
        // A view, not a setting: the same file, the same cut, half the height —
        // and on a phone that half is the difference between reading the plan
        // and scrolling for it.
        val raw = EngineTest.wav(bars = 8)
        val analysis = Engine.analyze(org.loopslcr.Native.direct(raw), "200 loop.wav")
        val peaks = Engine.peaks(org.loopslcr.Native.direct(raw), 512)
        val plan = Engine.plan(org.loopslcr.Native.direct(raw), "200 loop.wav", Settings())

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                CutterScreen(
                    loaded = Loaded(name, org.loopslcr.Native.direct(raw), analysis, peaks),
                    settings = Settings(),
                    plan = plan,
                    busy = Busy.Idle,
                    problem = null,
                    onOpen = {},
                    onExport = {},
                    onChange = {},
                    onDismissProblem = {},
                )
            }
        }

        val tall = compose.onNodeWithTag("wave").getBoundsInRoot()
        compose.onNodeWithTag("lanes").performClick()
        compose.waitForIdle()
        val short = compose.onNodeWithTag("wave").getBoundsInRoot()

        val was = tall.bottom - tall.top
        val now = short.bottom - short.top
        assertTrue("$was tall before, $now after", now < was * 0.7f)
        // Still the same file underneath, which is the half of "a view" that a
        // height check cannot see.
        compose.onNodeWithText(name).assertExists()
    }

    @Test
    fun the_target_bpm_slider_is_there_even_before_the_first_plan() {
        // Reported as "I switch to BPM and there is no slider". It hung on
        // `plan?.tempo`, and the plan is null before the first one lands and
        // again whenever the settings do not describe a cut — so the control
        // vanished, with nothing said. The file's own tempo is a perfectly good
        // answer and was sitting right there.
        val raw = EngineTest.wav(bars = 8)
        val analysis = Engine.analyze(org.loopslcr.Native.direct(raw), "200 loop.wav")
        val peaks = Engine.peaks(org.loopslcr.Native.direct(raw), 512)

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                CutterScreen(
                    loaded = Loaded(name, org.loopslcr.Native.direct(raw), analysis, peaks),
                    settings = Settings(speedMode = SpeedMode.TargetBpm),
                    plan = null,
                    busy = Busy.Idle,
                    problem = null,
                    onOpen = {},
                    onExport = {},
                    onChange = {},
                    onDismissProblem = {},
                )
            }
        }

        // 200 BPM from the name, so the span is 100..400 and the slider stands.
        compose.onNodeWithTag("bpmSlider").assertExists()
        compose.onNodeWithText("no source tempo yet", substring = true).assertDoesNotExist()
    }

    @Test
    fun the_master_switch_stands_beside_the_slider_and_only_when_there_are_two_loops() {
        // It was two chips naming both loops, on a row of their own. With one
        // loop there is nothing to be master *of*, so the switch is absent
        // rather than dead — and where it belongs is against the control it
        // governs, not on a line spent saying so.
        val raw = EngineTest.wav(bars = 8)
        val analysis = Engine.analyze(org.loopslcr.Native.direct(raw), "200 loop.wav")
        val peaks = Engine.peaks(org.loopslcr.Native.direct(raw), 512)
        var master by mutableStateOf<Boolean?>(null)
        var asked: Boolean? = null

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                CutterScreen(
                    loaded = Loaded(name, org.loopslcr.Native.direct(raw), analysis, peaks),
                    settings = Settings(),
                    plan = null,
                    busy = Busy.Idle,
                    problem = null,
                    master = master,
                    onMaster = { on -> asked = on },
                    onOpen = {},
                    onExport = {},
                    onChange = {},
                    onDismissProblem = {},
                )
            }
        }

        // No second loop: not there at all.
        compose.onNodeWithTag("master").assertDoesNotExist()

        compose.runOnIdle { master = false }

        val switch = compose.onNodeWithTag("master").getBoundsInRoot()
        val slider = compose.onNodeWithTag("semitoneSlider").getBoundsInRoot()
        // Beside, not above: the same row, and the slider to its right.
        assertTrue(
            "MSTR at ${switch.top}..${switch.bottom}, slider at ${slider.top}..${slider.bottom}",
            switch.top < slider.bottom && slider.top < switch.bottom,
        )
        // Left of it, and the slider takes the rest of the row. Compared on the
        // left edges: a slider's node reaches a couple of dp back for its thumb,
        // so the two touch targets legitimately overlap at the seam.
        assertTrue(
            "MSTR from ${switch.left}, slider from ${slider.left} to ${slider.right}",
            switch.left < slider.left && slider.right > switch.right,
        )

        compose.onNodeWithTag("master").performClick()
        assertEquals("off asks to be turned on", true, asked)
    }

    @Test
    fun the_master_holds_a_tempo_and_lets_go_of_one_moved_by_hand() {
        // The rule the switch is made of. On, the pair runs at that loop's
        // tempo and follows it when it changes; the moment the speed is moved
        // by hand it is no longer that loop's tempo, and the switch has to
        // notice rather than put the number back.
        val at = Settings(speedMode = SpeedMode.TargetBpm, targetBpm = 93.0)
        assertTrue(holdsTempo(at, 93.0))
        assertTrue("a rounding step is not a hand", holdsTempo(at.copy(targetBpm = 93.0005), 93.0))
        assertTrue("a drag is", !holdsTempo(at.copy(targetBpm = 94.0), 93.0))
        assertTrue("so is switching units", !holdsTempo(at.copy(speedMode = SpeedMode.Semitones), 93.0))
        assertTrue("a loop with no tempo cannot be the reference", !holdsTempo(at, null))
    }

    @Test
    fun the_waveform_and_the_varispeed_do_not_scroll_away() {
        // The whole screen used to scroll as one, so reaching any control
        // pushed the picture it acted on off the top: you could change the
        // thing or watch it, never both.
        val raw = EngineTest.wav(bars = 8)
        val analysis = Engine.analyze(org.loopslcr.Native.direct(raw), "200 loop.wav")
        val peaks = Engine.peaks(org.loopslcr.Native.direct(raw), 512)
        val plan = Engine.plan(org.loopslcr.Native.direct(raw), "200 loop.wav", Settings())

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                CutterScreen(
                    loaded = Loaded(name, org.loopslcr.Native.direct(raw), analysis, peaks),
                    settings = Settings(),
                    plan = plan,
                    busy = Busy.Idle,
                    problem = null,
                    onOpen = {},
                    onExport = {},
                    onChange = {},
                    onDismissProblem = {},
                )
            }
        }

        // Open every group first. With everything folded the screen fits on an
        // emulator, so there is nothing to scroll and the test cannot tell the
        // layouts apart — it passed against the old one until this was added.
        for (group in listOf("SOURCE", "LOOP", "TAPE", "OUTPUT")) {
            compose.onNodeWithText(group).performClick()
            compose.waitForIdle()
        }

        compose.onNodeWithText("semitones").assertIsDisplayed()
        val before = compose.onNodeWithText("semitones").getBoundsInRoot()

        // Scroll the lower half to its end, which is as far as it can go.
        compose.onNodeWithText("Export").performScrollTo()
        compose.waitForIdle()

        compose.onNodeWithText("semitones").assertIsDisplayed()
        assertEquals(before.top, compose.onNodeWithText("semitones").getBoundsInRoot().top)
    }

    @Test
    fun the_calculator_renders_its_table() {
        val settings = CalculatorSettings(bpm = "103", sampleRate = 44_100, bars = 8)
        val sums = Calculator.compute(settings).getOrThrow()

        compose.setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Palette.wave)) {
                CalculatorScreen(settings = settings, sums = sums, problem = null, onChange = {})
            }
        }

        // Not "1/4": that is also a BPM-unit chip, and a selector that matches
        // two different things is a selector that will pass for the wrong reason.
        compose.onNodeWithText("1/16T").assertExists()
        compose.onNodeWithText("NOTE VALUES").assertExists()
        // 103 BPM divides no sample rate cleanly, so every row should be marked
        // inexact — the one fact on the screen that is not decoration.
        assertTrue(sums.notes.none { it.sampleExact })

        save(compose.onRoot().captureToImage().asAndroidBitmap(), "calculator.png")
    }

    private fun save(bitmap: Bitmap, name: String) {
        val dir = InstrumentationRegistry.getInstrumentation().targetContext.filesDir
        File(dir, name).outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }
}
