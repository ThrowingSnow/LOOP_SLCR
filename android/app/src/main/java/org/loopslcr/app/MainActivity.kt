package org.loopslcr.app

import android.net.Uri
import android.os.Bundle
import android.provider.OpenableColumns
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.lifecycle.ViewModelProvider
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive

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
        val bytes = runCatching {
            contentResolver.openInputStream(uri)?.use { it.readBytes() }
        }.getOrNull()
        if (bytes == null) {
            model.fail("could not read ${displayName(uri)}")
            return@registerForActivityResult
        }
        model.open(displayName(uri), bytes)
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

                CutterScreen(
                    loaded = loaded,
                    settings = settings,
                    plan = plan,
                    busy = busy,
                    problem = problem,
                    playing = playing,
                    playHead = head,
                    onPlay = { model.togglePlay() },
                    onOpen = {
                        // Not `audio/*`: a WAVE file that a device has decided is
                        // `application/octet-stream` would be unpickable, and the
                        // native side rejects anything that is not RIFF anyway.
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
