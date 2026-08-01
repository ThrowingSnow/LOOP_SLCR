package org.loopslcr.app

import android.net.Uri
import android.os.Bundle
import android.provider.OpenableColumns
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Tab
import androidx.compose.material3.TabRow
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.lifecycle.ViewModelProvider
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import java.nio.ByteBuffer

/**
 * The one activity.
 *
 * It owns the two things that can only be done from an activity — picking a file
 * to read and picking a place to write — and hands everything else to the view
 * model. Nothing here decides anything about audio.
 */
class MainActivity : ComponentActivity() {
    private lateinit var model: CutterViewModel

    /**
     * Reading: `OpenDocument` rather than a permission.
     *
     * The user picks one file and the app is granted that file. It never asks
     * for access to storage, so there is no version of this app that has read
     * anything the user did not hand it.
     */
    private val openFile = registerForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        uri ?: return@registerForActivityResult
        val name = displayName(uri)
        val loaded = runCatching { read(uri) }
        loaded.fold(
            onSuccess = { buffer ->
                if (buffer == null) model.fail("could not read $name") else model.open(name, buffer)
            },
            onFailure = { model.fail(explain(it, "could not read $name")) },
        )
    }

    /**
     * Reads the file into memory Rust can use without a second copy.
     *
     * The file is the biggest thing in this app by a wide margin, so it is worth
     * one round trip to the provider to learn its size: with the size known it
     * goes straight into a direct buffer of exactly that length, and the Java
     * heap never holds a copy at all. Without it, the bytes are read the ordinary
     * way and copied once — correct, just twice the peak.
     *
     * The buffer's *capacity* is what the native side reads, so it has to be
     * exactly the file: a buffer with slack would hand Rust trailing zeroes as
     * though they were audio.
     */
    private fun read(uri: Uri): ByteBuffer? {
        val size = sizeOf(uri)
        contentResolver.openInputStream(uri).use { stream ->
            if (stream == null) return null
            if (size == null || size <= 0 || size > Int.MAX_VALUE) {
                val bytes = stream.readBytes()
                return ByteBuffer.allocateDirect(bytes.size).put(bytes).rewind() as ByteBuffer
            }
            val buffer = ByteBuffer.allocateDirect(size.toInt())
            val chunk = ByteArray(64 * 1024)
            while (buffer.hasRemaining()) {
                val read = stream.read(chunk, 0, minOf(chunk.size, buffer.remaining()))
                if (read <= 0) break
                buffer.put(chunk, 0, read)
            }
            if (buffer.hasRemaining()) {
                // The provider's size was a lie, or the file shrank mid-read.
                // Handing the native side a partly filled buffer would present
                // the unwritten tail as silence, so say so instead.
                return null
            }
            return buffer.rewind() as ByteBuffer
        }
    }

    private fun sizeOf(uri: Uri): Long? {
        contentResolver.query(uri, arrayOf(OpenableColumns.SIZE), null, null, null)?.use { cursor ->
            if (cursor.moveToFirst() && !cursor.isNull(0)) return cursor.getLong(0)
        }
        return null
    }

    /** Writing: the same, in the other direction. */
    private var pendingExport: ByteArray? = null
    private val createFile = registerForActivityResult(
        ActivityResultContracts.CreateDocument("audio/wav"),
    ) { uri ->
        val bytes = pendingExport
        pendingExport = null
        if (uri == null || bytes == null) return@registerForActivityResult
        runCatching {
            contentResolver.openOutputStream(uri)?.use { it.write(bytes) }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        model = ViewModelProvider(this)[CutterViewModel::class.java]

        setContent {
            MaterialTheme(
                colorScheme = darkColorScheme(
                    primary = Palette.wave,
                    background = Palette.background,
                    surface = Palette.surface,
                ),
            ) {
                val loaded by model.loaded.collectAsState()
                val settings by model.settings.collectAsState()
                val plan by model.plan.collectAsState()
                val busy by model.busy.collectAsState()
                val problem by model.problem.collectAsState()
                val playing by model.playing.collectAsState()

                // Polled rather than pushed. The audio thread publishes its
                // position to an atomic and must not be made to notify anyone;
                // asking it thirty times a second is both cheaper and, for a
                // moving cursor, indistinguishable.
                var head by remember { mutableStateOf<Float?>(null) }
                LaunchedEffect(playing, plan, loaded) {
                    val file = loaded
                    val where = plan
                    if (!playing || file == null || where == null || file.analysis.frames == 0L) {
                        head = null
                        return@LaunchedEffect
                    }
                    while (isActive) {
                        val position = model.playPosition()
                        head = position?.let {
                            ((where.regionStart + it) / file.analysis.frames).toFloat()
                        }
                        delay(33)
                    }
                }

                val calculator by model.calculator.collectAsState()
                val sums by model.sums.collectAsState()
                val calculatorProblem by model.calculatorProblem.collectAsState()
                var tab by remember { mutableIntStateOf(0) }

                // The insets are consumed once, here, so the tab bar clears the
                // status bar and the screens below inherit a clean area. Doing it
                // in both places padded everything twice.
                Column(
                    Modifier
                        .fillMaxSize()
                        .background(Palette.background)
                        .windowInsetsPadding(WindowInsets.safeDrawing),
                ) {
                    TabRow(
                        selectedTabIndex = tab,
                        containerColor = Palette.background,
                        contentColor = Palette.wave,
                    ) {
                        Tab(tab == 0, onClick = { tab = 0 }) {
                            Text("CUTTER", Modifier.padding(12.dp), fontSize = 12.sp)
                        }
                        Tab(tab == 1, onClick = { tab = 1 }) {
                            Text("CALCULATOR", Modifier.padding(12.dp), fontSize = 12.sp)
                        }
                    }

                    when (tab) {
                        0 -> CutterScreen(
                            loaded = loaded,
                            settings = settings,
                            plan = plan,
                            busy = busy,
                            problem = problem,
                            playing = playing,
                            playHead = head,
                            onPlay = { model.togglePlay() },
                            onDragMarker = { marker, at -> model.dragMarker(marker, at) },
                            onOpen = {
                                // Not `audio/*`: a WAVE file a device has decided
                                // is `application/octet-stream` would be
                                // unpickable, and the native side rejects
                                // anything that is not RIFF anyway.
                                openFile.launch(arrayOf("*/*"))
                            },
                            onExport = {
                                model.export { bytes ->
                                    pendingExport = bytes
                                    createFile.launch(model.suggestedName())
                                }
                            },
                            onChange = { change -> model.update(change) },
                            onDismissProblem = { model.dismissProblem() },
                        )

                        else -> CalculatorScreen(
                            settings = calculator,
                            sums = sums,
                            problem = calculatorProblem,
                            onSendToCutter = {
                                model.sendTempoToCutter()
                                tab = 0
                            },
                            onChange = { change -> model.updateCalculator(change) },
                        )
                    }
                }
            }
        }
    }

    /**
     * The name the user knows the file by.
     *
     * It matters more than it looks: the tempo and the bar count are read off
     * the file name when the file itself does not declare them, so a document
     * URI's opaque id would lose information the pipeline uses.
     */
    private fun displayName(uri: Uri): String {
        contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
            ?.use { cursor ->
                if (cursor.moveToFirst() && !cursor.isNull(0)) return cursor.getString(0)
            }
        return uri.lastPathSegment ?: "loop.wav"
    }
}
