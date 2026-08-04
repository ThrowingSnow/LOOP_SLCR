package org.loopslcr.app

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlin.coroutines.cancellation.CancellationException
import java.nio.ByteBuffer
import kotlin.math.abs


/** How many waveform buckets to ask for. Redrawn on resize, not re-measured. */
private const val BUCKETS = 4096

/** How long a setting must sit still before the plan is recomputed. */
private const val REPLAN_DELAY_MS = 250L

data class Loaded(
    val name: String,
    /** The file, in memory the JVM owns and Rust reads in place. */
    val bytes: ByteBuffer,
    val analysis: Analysis,
    val peaks: FloatArray,
) {
    // Data classes compare arrays by identity, which for a loaded file is what
    // is wanted — but `equals` and `hashCode` have to agree about it, and the
    // compiler warns unless both are spelled out.
    override fun equals(other: Any?) = this === other
    override fun hashCode() = System.identityHashCode(this)
}

sealed interface Busy {
    data object Idle : Busy
    data class Working(val what: String) : Busy
}

class CutterViewModel : ViewModel() {
    private val _loaded = MutableStateFlow<Loaded?>(null)
    val loaded: StateFlow<Loaded?> = _loaded.asStateFlow()

    private val _settings = MutableStateFlow(Settings())
    val settings: StateFlow<Settings> = _settings.asStateFlow()

    private val _plan = MutableStateFlow<Plan?>(null)
    val plan: StateFlow<Plan?> = _plan.asStateFlow()

    private val _busy = MutableStateFlow<Busy>(Busy.Idle)
    val busy: StateFlow<Busy> = _busy.asStateFlow()

    /** The last thing that went wrong, for the UI to show and then dismiss. */
    private val _problem = MutableStateFlow<String?>(null)
    val problem: StateFlow<String?> = _problem.asStateFlow()

    private var replan: Job? = null

    // --- the calculator tab ------------------------------------------------
    //
    // It lives in the same view model rather than its own, because the two tabs
    // share facts: a loaded file's tempo and sample rate seed the calculator,
    // and a tempo worked out over there is meant to be sent back here.

    private val _calculator = MutableStateFlow(CalculatorSettings())
    val calculator: StateFlow<CalculatorSettings> = _calculator.asStateFlow()

    private val _sums = MutableStateFlow<Sums?>(null)
    val sums: StateFlow<Sums?> = _sums.asStateFlow()

    private val _calculatorProblem = MutableStateFlow<String?>(null)
    val calculatorProblem: StateFlow<String?> = _calculatorProblem.asStateFlow()

    init {
        recalculate()
    }

    fun updateCalculator(change: (CalculatorSettings) -> CalculatorSettings) {
        _calculator.update(change)
        recalculate()
    }

    /**
     * Sends the calculator's tempo to the cutter as a varispeed target.
     *
     * The one direction that means something: the calculator is where you work
     * out what tempo you want, and the cutter is where a loop is made to arrive
     * at it.
     */
    fun sendTempoToCutter() {
        val bpm = _calculator.value.bpm.toDoubleOrNull() ?: return
        update { it.copy(speedMode = SpeedMode.TargetBpm, targetBpm = bpm) }
    }

    private fun recalculate() {
        // Exact rational arithmetic over eighteen note values costs microseconds
        // and allocates one string. Pushing it to another thread would add more
        // latency in scheduling than it removes in work.
        Calculator.compute(_calculator.value)
            .onSuccess {
                _sums.value = it
                _calculatorProblem.value = null
            }
            .onFailure {
                _sums.value = null
                _calculatorProblem.value = it.message ?: "those numbers do not describe a grid"
            }
    }

    private val player = PreviewPlayer()
    private val _playing = MutableStateFlow(false)
    val playing: StateFlow<Boolean> = _playing.asStateFlow()

    /** The settings the running preview was built from, to know when to rebuild. */
    private var playingSettings: Settings? = null

    /**
     * One preview build at a time.
     *
     * Building runs the whole pipeline, which on a phone takes longer than the
     * replan debounce — so a control that changes the loop *continuously* can
     * ask for a second build while the first is still running. Only the tape
     * sliders can: every other setting that invalidates a preview is a chip or a
     * toggle, and speed does not invalidate it at all. That is why this bug
     * belonged to tape and to nothing else.
     *
     * The lock rather than cancellation alone, because a cancelled coroutine
     * does not abort a native call already in flight: it returns, throws on the
     * way out, and leaves a built preview behind. Queueing them means the later
     * build always installs last, which is the one the user asked for.
     */
    private val building = Mutex()
    private var rebuild: Job? = null

    override fun onCleared() {
        player.stop()
        super.onCleared()
    }

    /**
     * Starts or stops the preview.
     *
     * Starting is not instant — the pipeline runs first — so it happens off the
     * main thread like everything else that touches the engine.
     */
    fun togglePlay() {
        if (_playing.value) {
            partnering?.cancel()
            player.stop()
            _playing.value = false
            playingSettings = null
            return
        }
        val file = _loaded.value ?: return
        val wanted = _settings.value
        rebuild?.cancel()
        rebuild = viewModelScope.launch {
            _busy.value = Busy.Working("starting the preview")
            val ratio = _plan.value?.ratio ?: 1.0
            val problem = building.withLock {
                withContext(Dispatchers.Default) {
                    player.start(file.bytes, file.name, wanted, ratio)
                }
            }
            _busy.value = Busy.Idle
            if (problem == null) {
                _playing.value = true
                playingSettings = wanted
                // A fresh handle knows nothing about the motion the user left
                // switched on, and a control that silently stops applying when
                // playback restarts reads as a broken control. The same goes
                // for the second loop, which lives in the handle and dies with
                // it.
                pushMotion()
                pushPair()
                pushPartner()
                // The gains live in the handle too, and a fresh one starts at
                // unity — so a level the user set would silently jump back.
                player.setGains(_gains.value.first, _gains.value.second)
                player.setMasterGain(_masterGain.value)
                pushFx()
            } else {
                _problem.value = problem
            }
        }
    }

    /**
     * Where the play head is, for a UI that wants to draw it.
     *
     * The *sounding* position, not the clock: with a motion running the two are
     * different, and the one worth drawing is the part of the file you can
     * actually hear. A head that ignored the displacement would calmly sweep
     * left to right while the loop jumped around underneath it.
     */
    fun playPosition(): Double? = player.sounding()

    // --- the second loop ---------------------------------------------------
    //
    // A whole second deck would be two of everything; this is deliberately not
    // that. The second loop has no varispeed and no export of its own, because
    // it does not have a length of its own: it is pulled to the first loop's
    // tempo and bar count so the two can share one play head. What it does have
    // is a file, a plan, and which part of it to use.

    private val _second = MutableStateFlow<Loaded?>(null)
    val second: StateFlow<Loaded?> = _second.asStateFlow()

    private val _secondSettings = MutableStateFlow(Settings())
    val secondSettings: StateFlow<Settings> = _secondSettings.asStateFlow()

    private val _secondPlan = MutableStateFlow<Plan?>(null)
    val secondPlan: StateFlow<Plan?> = _secondPlan.asStateFlow()

    /** What went wrong with the second loop, which is usually its length. */
    private val _secondProblem = MutableStateFlow<String?>(null)
    val secondProblem: StateFlow<String?> = _secondProblem.asStateFlow()

    private val _pair = MutableStateFlow(PairSettings())
    val pair: StateFlow<PairSettings> = _pair.asStateFlow()

    private var partnering: Job? = null

    /**
     * The parameters that make the second file fit the first.
     *
     * This is the whole trick: the second loop is cut to the *first* loop's bar
     * count and then to the first loop's exact length in frames, with the same
     * rational arithmetic as any other cut. What comes out is the same number of
     * frames, so one play head can serve both.
     *
     * **The length, not the tempo.** It was the tempo, and it was wrong by six
     * frames on a real pair of loops: the tempo route rounds twice on the way to
     * a length — once to a bar grid, once to a frame — and the first loop's own
     * length had been reached by a different path. Two loops that agree about
     * the tempo can still disagree about the sample, and a shared play head has
     * no room for that disagreement. The tempo still goes along, because it is
     * what makes the ratio sane and is what the plan reports; the length is what
     * has to come true.
     *
     * The first loop's *source* tempo and *unstretched* length, not its target:
     * the preview plays the cut at its own tempo and the varispeed is applied
     * live to the whole handle, so both loops are already moving together by the
     * time a ratio is involved.
     */
    private fun partnerSettings(): Settings? {
        val mine = _plan.value ?: return null
        val theirs = _secondSettings.value
        if (mine.bars <= 0) return null
        return theirs.copy(
            bars = mine.bars,
            speedMode = SpeedMode.TargetBpm,
            targetBpm = mine.tempo,
            snap = false,
            targetFrames = firstLength(),
        )
    }

    /**
     * How long the first loop is, in frames, as the second one must match it.
     *
     * From the running preview when there is one — that buffer *is* what the
     * partner joins, so nothing beats asking it — and from the plan otherwise,
     * so the dry run on CUTTER 2 shows the same numbers the audio will use.
     */
    private fun firstLength(): Long? = player.frames() ?: _plan.value?.loopFrames

    fun openSecond(name: String, bytes: ByteBuffer) {
        viewModelScope.launch {
            _busy.value = Busy.Working("reading $name")
            try {
                val loaded = withContext(Dispatchers.Default) {
                    val analysis = Engine.analyze(bytes, name)
                    Loaded(name, bytes, analysis, Engine.peaks(bytes, BUCKETS))
                }
                _second.value = loaded
                _secondSettings.value = Settings()
                _secondProblem.value = null
                planSecond()
                pushPartner()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Throwable) {
                _second.value = null
                _secondPlan.value = null
                _secondProblem.value = explain(e, "could not read the second file")
            } finally {
                _busy.value = Busy.Idle
            }
        }
    }

    fun updateSecond(change: (Settings) -> Settings) {
        _secondSettings.update(change)
        planSecond()
        pushPartner()
    }

    fun dropSecond() {
        partnering?.cancel()
        _second.value = null
        _secondPlan.value = null
        _secondProblem.value = null
        _pair.update { it.copy(on = false) }
        // Nothing left to be master of, and nothing left for loop 2 to be.
        _master.value = null
        player.clearPartner()
        pushPair()
    }

    fun setPair(change: (PairSettings) -> PairSettings) {
        _pair.update(change)
        pushPair()
    }

    private fun pushPair() {
        val p = _pair.value
        val steps = p.steps(_plan.value?.bars)
        if (steps == null) {
            player.setPair(false, 0, 1, 1)
            return
        }
        player.setPair(p.on, steps, p.holdA, p.holdB)
    }

    /**
     * Builds the second loop against the first and hands it to the audio.
     *
     * Queued behind the same lock the preview build uses: both run the pipeline,
     * and two pipeline runs at once on a phone is how the tape sliders once made
     * the loop play over itself.
     */
    private fun pushPartner() {
        val file = _second.value ?: return
        val wanted = partnerSettings() ?: return
        if (!_playing.value) return
        partnering?.cancel()
        partnering = viewModelScope.launch {
            val problem = building.withLock {
                withContext(Dispatchers.Default) {
                    player.setPartner(file.bytes, file.name, wanted)
                }
            }
            _secondProblem.value = problem
            if (problem == null) pushPair()
        }
    }

    private fun planSecond() {
        val file = _second.value ?: return
        val wanted = partnerSettings() ?: _secondSettings.value
        viewModelScope.launch {
            try {
                _secondPlan.value = withContext(Dispatchers.Default) {
                    Engine.plan(file.bytes, file.name, wanted)
                }
                applyMaster()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Throwable) {
                _secondPlan.value = null
                _secondProblem.value = explain(e, "the second loop cannot be cut that way")
            }
        }
    }

    // --- the mixer ---------------------------------------------------------

    private val _gains = MutableStateFlow(1f to 1f)
    val gains: StateFlow<Pair<Float, Float>> = _gains.asStateFlow()

    fun setGains(first: Float, second: Float) {
        _gains.value = first to second
        player.setGains(first, second)
    }

    /**
     * The trim on the sum.
     *
     * Kept apart from the two loop gains all the way down: a master move that
     * rewrote both channel faders would be a console lying about where its own
     * levels are.
     */
    private val _masterGain = MutableStateFlow(1f)
    val masterGain: StateFlow<Float> = _masterGain.asStateFlow()

    fun setMasterGain(gain: Float) {
        _masterGain.value = gain
        player.setMasterGain(gain)
    }

    /**
     * The insert on the sum, between the channel faders and the master.
     *
     * Held here rather than in [Settings] because it is not part of a cut: it
     * changes what you hear, not what the file is. Pushed whole, so the audio
     * thread never sees half a panel.
     */
    private val _fx = MutableStateFlow(Fx())
    val fx: StateFlow<Fx> = _fx.asStateFlow()

    fun setFx(fx: Fx) {
        _fx.value = fx
        pushFx()
    }

    /**
     * Hands the panel down, with the two numbers only this layer can work out.
     *
     * A division is per bar and the preview counts samples, so somebody has to
     * know the loop's bar count and its sample rate. That somebody is here: the
     * panel is a musical statement, the delay is an arithmetic one, and this is
     * where the two meet. Called again whenever the plan changes, because a
     * different bar count is a different length for the same eighth note.
     */
    private fun pushFx() {
        val fx = _fx.value
        val bars = _plan.value?.bars ?: 0L
        val rate = _loaded.value?.analysis?.sampleRate ?: 48_000
        player.setFx(fx, fx.syncFraction(bars), fx.freeSamples(rate))
    }

    /** The three meters from one block: loop 1, loop 2, and what left. */
    fun levels(): Triple<Float, Float, Float> = player.peaks()

    // --- which loop the pair's speed is measured against ------------------

    /** 1 or 2 while a loop is the reference, null while the speed is free. */
    private val _master = MutableStateFlow<Int?>(null)
    val master: StateFlow<Int?> = _master.asStateFlow()

    /**
     * Makes one loop the pair's speed reference — or lets it go.
     *
     * There is one speed, because there is one play head. MSTR does not give a
     * loop a speed of its own; it says which loop's tempo the pair runs at, and
     * so which one is pulled to the other. **At most one deck can hold it**:
     * turning it on here turns it off there, because two references is not a
     * state that means anything.
     *
     * Held rather than pressed once, so that a tempo that changes later — a bar
     * count edited, a source tempo typed in — carries the pair with it. What
     * keeps that from fighting the finger is [releaseMaster]: any hand-moved
     * speed drops the reference rather than being overwritten by it.
     */
    fun setMaster(deck: Int, on: Boolean) {
        if (!on) {
            if (_master.value == deck) _master.value = null
            return
        }
        if (masterTempo(deck) == null) {
            _problem.value = "loop $deck has no tempo yet — set one under SOURCE"
            return
        }
        _master.value = deck
        applyMaster()
    }

    private fun masterTempo(deck: Int? = _master.value): Double? = when (deck) {
        1 -> _plan.value?.tempo
        2 -> _secondPlan.value?.tempo
        else -> null
    }

    /**
     * Pulls the pair to the reference loop's tempo.
     *
     * Silent when it already is there — not an optimisation but a stop: this
     * runs when a plan lands, and a plan lands because of an update, so putting
     * an update at the end of it unconditionally is a loop that never settles.
     */
    private fun applyMaster() {
        val tempo = masterTempo() ?: return
        if (holdsTempo(_settings.value, tempo)) return
        update { it.copy(speedMode = SpeedMode.TargetBpm, targetBpm = tempo) }
    }

    /** Drops the reference as soon as the speed stops being that loop's tempo. */
    private fun releaseMaster() {
        val tempo = masterTempo() ?: return
        if (!holdsTempo(_settings.value, tempo)) _master.value = null
    }

    private val _motion = MutableStateFlow(MotionSettings())
    val motion: StateFlow<MotionSettings> = _motion.asStateFlow()

    /**
     * Changes the stepped displacement.
     *
     * **No replan, no rebuild, no debounce.** The motion never reaches the
     * pipeline — it is a way of reading the finished loop — so it goes straight
     * to the audio and takes effect at the next step boundary. This is the one
     * control on the screen that costs nothing at all to turn.
     */
    fun setMotion(change: (MotionSettings) -> MotionSettings) {
        _motion.update(change)
        pushMotion()
    }

    /**
     * Hands the current motion to the audio.
     *
     * Called on every change, and again whenever playback starts or the plan
     * lands, because the grid is the loop divided — so a loop that turned out
     * to be a different number of bars is a different grid for the same setting.
     */
    private fun pushMotion() {
        val m = _motion.value
        val steps = m.steps(_plan.value?.bars)
        if (steps == null) {
            player.setMotion(false, 0, 0, 1, 0)
            return
        }
        player.setMotion(m.on, steps, m.depth, m.every(), m.shape.ordinal)
    }

    /**
     * Moves one end of the cut, by finger.
     *
     * **The markers snap to bar lines**, and that is not a convenience — it is
     * the only way a drag can be allowed to touch this at all. The cut points
     * come from exact rational arithmetic over a bar index; letting a fingertip
     * name an arbitrary frame would hand the loop a length that no tempo
     * divides, which is precisely the drift this tool exists to remove. So a
     * drag chooses a *bar*, and the grid does the rest.
     *
     * The bar length used to turn a pixel into a bar is derived from the plan,
     * so it is a rounded number. That is fine: it is used for pointing, never
     * for cutting. Whatever bar the finger lands on, the cut itself is computed
     * from the exact grid.
     */
    fun dragMarker(marker: Marker, fraction: Float) {
        val file = _loaded.value ?: return
        val plan = _plan.value ?: return
        if (plan.bars <= 0) return

        val perBar = plan.samplesPerBar ?: return
        if (perBar <= 0.0) return

        val bar = Markers.barAt(fraction, file.analysis.frames, perBar)
        val (skip, bars) = Markers.dragged(marker, bar, plan.skipBars, plan.bars)
        update { it.copy(skip = skip, bars = bars) }
    }

    /**
     * Keeps a running preview in step with the settings.
     *
     * A change of speed is pushed to the handle and glides. Anything else
     * changes what is being played, so the preview is rebuilt — silently, since
     * the user did not ask for a stop, they asked for four bars instead of eight.
     */
    private fun followPreview(plan: Plan?) {
        if (!_playing.value) return
        val wanted = _settings.value
        val built = playingSettings
        if (built != null && !built.sameLoopAs(wanted)) {
            val file = _loaded.value ?: return
            // Where the loop had got to. A rebuild that starts from zero
            // retriggers the sound on every notch of a slider, which is heard as
            // stuttering rather than as an adjustment. The length is unchanged
            // unless the bars changed, and `seek` wraps, so this is always a
            // position the new preview has.
            val at = player.position()
            rebuild?.cancel()
            rebuild = viewModelScope.launch {
                val ratio = plan?.ratio ?: 1.0
                val problem = building.withLock {
                    withContext(Dispatchers.Default) {
                        player.start(file.bytes, file.name, wanted, ratio)
                    }
                }
                if (problem == null) {
                    at?.let { player.seek(it) }
                    playingSettings = wanted
                    // Same reason as in `togglePlay`, and also because the loop
                    // may now be a different number of bars — which is a
                    // different grid for the same setting, and a different
                    // length for the second loop to be cut to.
                    pushMotion()
                    pushPair()
                    pushPartner()
                    player.setGains(_gains.value.first, _gains.value.second)
                    player.setMasterGain(_masterGain.value)
                    pushFx()
                } else {
                    _playing.value = false
                    playingSettings = null
                    _problem.value = problem
                }
            }
        } else {
            player.setRatio(plan?.ratio ?: 1.0)
            // The bars can change without the loop needing a rebuild, and the
            // grid is the loop divided — so the same setting is a different
            // number of pieces and has to be re-sent.
            pushMotion()
            pushPair()
        }
    }

    fun dismissProblem() {
        _problem.value = null
    }

    /** For failures that happen before the engine is reached, like an unreadable URI. */
    fun fail(message: String) {
        _problem.value = message
    }

    fun open(name: String, bytes: ByteBuffer) {
        viewModelScope.launch {
            // A new file is a different loop; whatever was playing is not it,
            // and neither is anything that was still being built for the old one.
            rebuild?.cancel()
            player.stop()
            _playing.value = false
            playingSettings = null

            _busy.value = Busy.Working("reading $name")
            try {
                val loaded = withContext(Dispatchers.Default) {
                    val analysis = Engine.analyze(bytes, name)
                    Loaded(name, bytes, analysis, Engine.peaks(bytes, BUCKETS))
                }
                _loaded.value = loaded
                // A file usually names its own length; starting from what it
                // says beats starting from a guess the user then has to undo.
                _settings.value = Settings()

                // The calculator follows the file it was opened next to. Its
                // defaults are a guess; the file is a fact.
                _calculator.update { settings ->
                    settings.copy(
                        bpm = loaded.analysis.tempo?.let(::trim) ?: settings.bpm,
                        sampleRate = loaded.analysis.sampleRate,
                        bars = loaded.analysis.loopBars ?: settings.bars,
                    )
                }
                recalculate()
                _plan.value = null
                schedulePlan(immediately = true)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Throwable) {
                _loaded.value = null
                _plan.value = null
                _problem.value = explain(e, "could not read the file")
            } finally {
                _busy.value = Busy.Idle
            }
        }
    }

    fun update(change: (Settings) -> Settings) {
        val before = _settings.value
        _settings.update(change)
        val after = _settings.value

        // Speed is the one setting a running preview can follow without being
        // rebuilt, so it must not wait for the plan. It used to: the ratio came
        // only from the planning call, behind a 250 ms debounce, so the slider
        // answered on release rather than under the finger. The exact value
        // still arrives with the plan and overwrites this one — see [Speed].
        if (_playing.value && before.sameLoopAs(after)) {
            Speed.ratio(after, sourceTempo())?.let { player.setRatio(it) }
        }

        // A hand on the speed outranks the MSTR switch — see [setMaster].
        releaseMaster()

        schedulePlan(immediately = false)
    }

    /** The tempo the loaded file is running at, as best as it is known. */
    private fun sourceTempo(): Double? =
        _plan.value?.tempo ?: _loaded.value?.analysis?.tempo

    /**
     * Recomputes the plan, after a pause if a slider is still moving.
     *
     * `plan` runs the whole pipeline — it resamples the file to find out what
     * the result would be — so it is not free and must not run per pixel of
     * drag. The delay is the difference between a UI that answers and one that
     * stutters.
     */
    private fun schedulePlan(immediately: Boolean) {
        val file = _loaded.value ?: return
        val wanted = _settings.value
        replan?.cancel()
        replan = viewModelScope.launch {
            if (!immediately) delay(REPLAN_DELAY_MS)
            try {
                val plan = withContext(Dispatchers.Default) {
                    Engine.plan(file.bytes, file.name, wanted)
                }
                _plan.value = plan
                _problem.value = null
                // The reference loop's tempo may have just moved under it.
                applyMaster()
                // And a different bar count is a different length for the same
                // eighth note, so a synced echo has to be told.
                pushFx()
                followPreview(plan)
            } catch (e: CancellationException) {
                // A newer change cancelled this one. That is the debounce
                // working, not a failure — and reporting it left "h1 was
                // cancelled" on screen, which is a `JobCancellationException`
                // with its class name obfuscated by R8. Rethrowing is also
                // required: swallowing a cancellation leaves the coroutine
                // machinery believing a cancelled job is still alive.
                throw e
            } catch (e: Throwable) {
                _plan.value = null
                _problem.value = explain(e, "the settings do not describe a cut")
            }
        }
    }

    /** Runs the cut and hands the bytes to [sink], which writes them out. */
    fun export(sink: (ByteArray) -> Unit) {
        val file = _loaded.value ?: return
        val wanted = _settings.value
        viewModelScope.launch {
            _busy.value = Busy.Working("cutting")
            try {
                val out = withContext(Dispatchers.Default) {
                    Engine.process(file.bytes, file.name, wanted)
                }
                sink(out)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Throwable) {
                _problem.value = explain(e, "the cut failed")
            } finally {
                _busy.value = Busy.Idle
            }
        }
    }

    /** `103 loop.wav` becomes `103 loop — cut.wav`, as on the command line. */
    fun suggestedName(): String {
        val name = _loaded.value?.name ?: return "loop.wav"
        val stem = name.substringBeforeLast('.', name)
        val plan = _plan.value
        val what = when {
            plan == null -> "cut"
            plan.ratio != 1.0 -> "${trim(plan.resultingTempo)}bpm"
            else -> "${plan.bars}bars"
        }
        return "$stem — $what.wav"
    }
}

/** `100.0` reads as `100`; `103.5` has to keep its half. */
fun trim(value: Double): String =
    if (value == value.toLong().toDouble()) value.toLong().toString() else "%.3f".format(value).trimEnd('0').trimEnd('.')

/**
 * Whether a speed setting still *is* that tempo.
 *
 * The single question both halves of MSTR ask: applying asks it to avoid
 * re-applying what is already there, releasing asks it to notice a hand on the
 * slider. Milli-BPM, because that is the resolution the tempo slider snaps to —
 * a tighter comparison would drop the reference on a rounding step nobody made.
 */
internal fun holdsTempo(s: Settings, tempo: Double?): Boolean =
    tempo != null && s.speedMode == SpeedMode.TargetBpm &&
        s.targetBpm?.let { abs(it - tempo) < 0.001 } == true
