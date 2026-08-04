package org.loopslcr.app

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import android.os.Process
import org.json.JSONObject
import org.loopslcr.Native
import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * The loop, playing, at a speed that can be changed while it plays.
 *
 * # The shape of it
 *
 * One thread does nothing but ask Rust for a block of floats and hand it to
 * `AudioTrack`. Everything else — starting, stopping, changing the rate —
 * happens on whatever thread called, and reaches the audio thread through the
 * native handle's atomics rather than through anything that could make it wait.
 *
 * # The two rules
 *
 * **The handle is destroyed only after the audio thread has stopped.** A read
 * from a freed handle is undefined behaviour, and the window is exactly as long
 * as one block — which is to say it would happen rarely, on a device, in a way
 * that looks like a random crash. So [stop] joins before it frees, every time.
 *
 * **Only one thread may start or stop at a time.** [start] begins by stopping
 * whatever was playing, and that read-then-replace runs over four fields. Two
 * callers interleaving in it is not a subtle race: the second one's `stop` finds
 * `pump` and `track` still null because the first has not assigned them yet,
 * tears down nothing, and builds a second `AudioTrack` with a second pump thread
 * beside the first. The fields then remember only the later of the two, so the
 * earlier one can never be stopped again — the loop plays over itself, out of
 * phase, until the process dies. It also leaves that orphaned thread reading a
 * handle the survivor's `stop` will free.
 *
 * Hence the monitor on both. It is reentrant, so `start` calling `stop` is fine.
 * It is held across a `join` of at most one block, and never across the pipeline
 * run — the caller does that before it gets here.
 */
class PreviewPlayer {
    private var handle = 0L
    private var track: AudioTrack? = null
    private var pump: Thread? = null

    @Volatile
    private var running = false

    val isPlaying: Boolean get() = running

    /** Frames per block. See [blockFrames] for why it is not simply "small". */
    private var blockFrames = 0

    /**
     * Opens a preview and starts playing it.
     *
     * Returns the error if there is one, rather than throwing: a preview that
     * cannot open is a message in the UI, not a crash.
     *
     * Two steps on purpose. Building runs the whole pipeline and takes as long
     * as a cut does; installing touches the fields and takes microseconds. Only
     * the second is under the monitor, because a tap on pause must not wait for
     * a preview it is trying to replace.
     */
    fun start(audio: ByteBuffer, name: String, settings: Settings, ratio: Double): String? {
        val built = try {
            Native.previewCreate(audio, name, settings.toJson())
        } catch (e: kotlin.coroutines.cancellation.CancellationException) {
            throw e
        } catch (e: Throwable) {
            stop()
            return explain(e, "the preview could not start")
        }
        return install(built, ratio)
    }

    /** Swaps a freshly built handle in for whatever was playing. */
    @Synchronized
    private fun install(fresh: Long, ratio: Double): String? {
        stop()
        // Before anything that can fail, so every failure path below reaches a
        // `stop` that frees it rather than leaking a loop's worth of audio.
        handle = fresh
        return try {
            val info = JSONObject(Native.previewInfo(handle))
            val channels = info.getInt("channels")
            val rate = info.getInt("sampleRate")
            Native.previewSetRatio(handle, ratio)

            val mask = when (channels) {
                1 -> AudioFormat.CHANNEL_OUT_MONO
                2 -> AudioFormat.CHANNEL_OUT_STEREO
                else -> throw IllegalStateException("$channels channels is not something to play")
            }
            val format = AudioFormat.Builder()
                .setEncoding(AudioFormat.ENCODING_PCM_FLOAT)
                .setSampleRate(rate)
                .setChannelMask(mask)
                .build()

            // The device's own minimum, doubled. The minimum is what it takes
            // not to underrun when nothing else is happening; on a phone that is
            // optimistic, and an underrun in a preview is a click at a random
            // place in the loop — the one artefact this whole tool exists to
            // avoid, arriving from the playback path instead of the audio.
            val minBytes = AudioTrack.getMinBufferSize(rate, mask, AudioFormat.ENCODING_PCM_FLOAT)
            val bytesPerFrame = channels * 4
            val trackBytes = maxOf(minBytes * 2, bytesPerFrame * 2048)
            blockFrames = (trackBytes / bytesPerFrame / 4).coerceAtLeast(128)

            val output = AudioTrack.Builder()
                .setAudioAttributes(
                    AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_MEDIA)
                        .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                        .build(),
                )
                .setAudioFormat(format)
                .setBufferSizeInBytes(trackBytes)
                .setTransferMode(AudioTrack.MODE_STREAM)
                .build()

            track = output
            output.play()
            running = true
            pump = Thread({ pump(channels) }, "loopslcr-preview").also { it.start() }
            null
        } catch (e: kotlin.coroutines.cancellation.CancellationException) {
            // Never a failure to report: the caller went away. Rethrown so the
            // coroutine that owns this one learns it was cancelled.
            stop()
            throw e
        } catch (e: Throwable) {
            stop()
            explain(e, "the preview could not start")
        }
    }

    /**
     * The audio thread.
     *
     * Nothing in here allocates after the first line. The block buffer is direct
     * so Rust writes into it in place, and it is reused for the life of the
     * playback: allocating per block would put the garbage collector on the one
     * thread that cannot afford to be paused.
     */
    private fun pump(channels: Int) {
        Process.setThreadPriority(Process.THREAD_PRIORITY_URGENT_AUDIO)
        val block = ByteBuffer
            .allocateDirect(blockFrames * channels * 4)
            .order(ByteOrder.nativeOrder())
        val audio = track ?: return

        while (running) {
            val frames = try {
                Native.previewRead(handle, block, blockFrames)
            } catch (_: IllegalStateException) {
                break
            }
            if (frames <= 0) break

            val bytes = frames * channels * 4
            block.position(0)
            block.limit(bytes)
            // WRITE_BLOCKING is what makes this loop self-pacing: it returns
            // when the device has room, so the thread runs at exactly the rate
            // the hardware consumes and needs no clock of its own.
            if (audio.write(block, bytes, AudioTrack.WRITE_BLOCKING) < 0) break
            block.clear()
        }
    }

    /** Asks for a new speed. Reached over the glide, not immediately. */
    fun setRatio(ratio: Double) {
        if (handle != 0L) {
            runCatching { Native.previewSetRatio(handle, ratio) }
        }
    }

    /**
     * Sets the stepped displacement of the play head, or turns it off.
     *
     * Lock-free like [setRatio], and for the same reason: this is a control
     * being turned while the audio thread is mid-block. It also does *not*
     * rebuild anything — the motion is a way of reading the loop, not a change
     * to the loop, so it costs nothing to turn and takes effect on the next
     * step boundary.
     */
    fun setMotion(on: Boolean, steps: Int, depth: Int, every: Int, shape: Int) {
        if (handle != 0L) {
            runCatching { Native.previewSetMotion(handle, on, steps, depth, every, shape) }
        }
    }

    /** Sets the swap schedule between the loop and its partner. Lock-free. */
    fun setPair(on: Boolean, steps: Int, holdA: Int, holdB: Int) {
        if (handle != 0L) {
            runCatching { Native.previewSetPair(handle, on, steps, holdA, holdB) }
        }
    }

    /**
     * Gives the running preview a second loop.
     *
     * Runs the pipeline, so it belongs on a background thread — and returns the
     * complaint rather than throwing, because "that loop is 352 800 frames
     * against 344 000" is a message in the UI, not a crash.
     *
     * Not `@Synchronized`: the native side does its own building outside its
     * lock, and holding the monitor here would make a stop wait for a pipeline
     * run. What it does need is a handle that is still open, which is the same
     * check every other call makes.
     */
    fun setPartner(audio: ByteBuffer, name: String, settings: Settings): String? {
        val open = handle
        if (open == 0L) return "nothing is playing to add a second loop to"
        return try {
            Native.previewSetPartner(open, audio, name, settings.toJson())
            null
        } catch (e: kotlin.coroutines.cancellation.CancellationException) {
            throw e
        } catch (e: Throwable) {
            explain(e, "the second loop could not be used")
        }
    }

    /** Sets the level trim for each loop, linear. Lock-free. */
    fun setGains(first: Float, second: Float) {
        if (handle != 0L) {
            runCatching { Native.previewSetGains(handle, first, second) }
        }
    }

    /** Sets the trim on the sum, after both loop gains. Lock-free. */
    fun setMasterGain(gain: Float) {
        if (handle != 0L) {
            runCatching { Native.previewSetMasterGain(handle, gain) }
        }
    }

    /**
     * The three meters, in one crossing of the boundary.
     *
     * Read together rather than one call each: they come from the same block,
     * and asking three times would let the master disagree with the channels it
     * is the sum of.
     */
    fun peaks(): Triple<Float, Float, Float> {
        if (handle == 0L) return Triple(0f, 0f, 0f)
        return runCatching {
            val info = JSONObject(Native.previewInfo(handle))
            Triple(
                info.getDouble("peakFirst").toFloat(),
                info.getDouble("peakSecond").toFloat(),
                info.getDouble("peakMaster").toFloat(),
            )
        }.getOrDefault(Triple(0f, 0f, 0f))
    }

    fun clearPartner() {
        if (handle != 0L) {
            runCatching { Native.previewClearPartner(handle) }
        }
    }

    /** Whether the second loop is the one being heard right now. */
    fun onSecond(): Boolean {
        if (handle == 0L) return false
        return runCatching { JSONObject(Native.previewInfo(handle)).getBoolean("onSecond") }
            .getOrDefault(false)
    }

    fun seek(frame: Double) {
        if (handle != 0L) {
            runCatching { Native.previewSeek(handle, frame) }
        }
    }

    /**
     * Where the clock is, in source frames, or null when not playing.
     *
     * Advances evenly whatever the motion is doing. This is the one to seek
     * back to: resuming at the displaced head would mean resuming somewhere the
     * loop had jumped to, which is not where the listener was.
     */
    fun position(): Double? = figure("position")

    /** Where the audio being heard comes from. What a play head should follow. */
    fun sounding(): Double? = figure("sounding")

    private fun figure(name: String): Double? {
        if (handle == 0L) return null
        return runCatching { JSONObject(Native.previewInfo(handle)).getDouble(name) }
            .getOrNull()
    }

    /**
     * Stops and frees, in that order.
     *
     * Idempotent, because a UI stops on pause, on a new file and on close, and
     * two of those regularly happen together.
     */
    @Synchronized
    fun stop() {
        running = false
        pump?.let {
            // Bounded: the thread is at most one blocking write from noticing,
            // and a write of one block is milliseconds. An unbounded join here
            // would hang the UI thread on a device whose audio path has wedged.
            it.join(2_000)
        }
        pump = null

        track?.let {
            runCatching { it.pause() }
            runCatching { it.flush() }
            runCatching { it.release() }
        }
        track = null

        // Only now: the audio thread is provably no longer reading it.
        if (handle != 0L) {
            runCatching { Native.previewDestroy(handle) }
            handle = 0L
        }
    }
}
