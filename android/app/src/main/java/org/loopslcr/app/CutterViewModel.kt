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
import kotlinx.coroutines.withContext

/** How many waveform buckets to ask for. Redrawn on resize, not re-measured. */
private const val BUCKETS = 512

/** How long a setting must sit still before the plan is recomputed. */
private const val REPLAN_DELAY_MS = 250L

data class Loaded(
    val name: String,
    val bytes: ByteArray,
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

    fun dismissProblem() {
        _problem.value = null
    }

    /** For failures that happen before the engine is reached, like an unreadable URI. */
    fun fail(message: String) {
        _problem.value = message
    }

    fun open(name: String, bytes: ByteArray) {
        viewModelScope.launch {
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
                _plan.value = null
                schedulePlan(immediately = true)
            } catch (e: Exception) {
                _loaded.value = null
                _plan.value = null
                _problem.value = e.message ?: "could not read the file"
            } finally {
                _busy.value = Busy.Idle
            }
        }
    }

    fun update(change: (Settings) -> Settings) {
        _settings.update(change)
        schedulePlan(immediately = false)
    }

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
            } catch (e: Exception) {
                _plan.value = null
                _problem.value = e.message ?: "the settings do not describe a cut"
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
            } catch (e: Exception) {
                _problem.value = e.message ?: "the cut failed"
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
