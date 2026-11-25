package cloud.fuss.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import cloud.fuss.core.FsRepository
import cloud.fuss.core.FussException
import kotlinx.coroutines.launch
import java.io.File

/** Archive formats the engine can write. */
private enum class OutFormat(val ext: String, val label: String) {
    ZIP("zip", "ZIP (deflate)"),
    TAR("tar", "TAR (uncompressed)"),
    TAR_GZ("tar.gz", "TAR + GZIP"),
}

/**
 * Archive creation for the current selection (or the current folder when
 * nothing is selected). The engine writes straight to the destination file;
 * nothing is staged in memory.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CompressScreen(
    directory: String,
    selection: List<String>,
    onBack: () -> Unit,
    onDone: (String) -> Unit,
) {
    val ctx = LocalContext.current
    val repo = remember { FsRepository() }
    val scope = rememberCoroutineScope()
    // Compressing a selection puts the archive next to the first source; the
    // current folder is used otherwise.
    val baseDir = if (selection.isNotEmpty()) File(selection.first()).parent ?: directory else directory
    val defaultName = when {
        selection.size == 1 -> File(selection.first()).name
        else -> File(baseDir).name.ifEmpty { "archive" }
    }
    var name by remember { mutableStateOf("$defaultName.zip") }
    var format by remember { mutableStateOf(OutFormat.ZIP) }
    var busy by remember { mutableStateOf(false) }
    var message by remember { mutableStateOf<String?>(null) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Create archive") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Outlined.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize().padding(16.dp)) {
            Text(
                if (selection.isEmpty()) "Compressing this folder" else "Compressing ${selection.size} selected item(s)",
                style = MaterialTheme.typography.bodyMedium,
            )
            Spacer(Modifier.height(12.dp))
            OutlinedTextField(
                value = name,
                onValueChange = { name = it },
                label = { Text("Archive name") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(Modifier.height(12.dp))
            OutFormat.entries.forEach { f ->
                Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
                    RadioButton(
                        selected = format == f,
                        onClick = {
                            format = f
                            name = name.substringBeforeLast('.', name) + "." + f.ext
                        },
                    )
                    Text(f.label)
                }
            }
            Spacer(Modifier.height(16.dp))
            Button(
                onClick = {
                    busy = true
                    message = null
                    scope.launch {
                        try {
                            val dest = File(baseDir, name).absolutePath
                            val sources = if (selection.isEmpty()) listOf(directory) else selection
                            val bytes = repo.createArchive(sources, dest, format.ext)
                            message = "Created ${File(dest).name} (${Format.size(bytes)})"
                            onDone(dest)
                        } catch (e: FussException) {
                            message = e.userMessage()
                        } finally {
                            busy = false
                        }
                    }
                },
                enabled = !busy && name.isNotBlank(),
                modifier = Modifier.fillMaxWidth(),
            ) { Text(if (busy) "Compressing…" else "Create") }
            Spacer(Modifier.height(8.dp))
            if (busy) BusyBanner("Writing archive")
            message?.let {
                Text(it, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(top = 8.dp))
            }
        }
    }
}
