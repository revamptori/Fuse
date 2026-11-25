package cloud.fuss.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.outlined.Folder
import androidx.compose.material.icons.outlined.FolderZip
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import cloud.fuss.core.ArchiveContents
import cloud.fuss.core.FsRepository
import cloud.fuss.core.FussException
import java.io.File

/**
 * Archive browser: lists a ZIP/TAR/TGZ/gz without extracting anything, then
 * extracts on demand into a folder next to the archive.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ArchiveScreen(
    archivePath: String,
    onBack: () -> Unit,
    onExtracted: (String) -> Unit,
) {
    val repo = remember { FsRepository() }
    var contents by remember(archivePath) { mutableStateOf<ArchiveContents?>(null) }
    var error by remember(archivePath) { mutableStateOf<String?>(null) }
    var extracting by remember { mutableStateOf(false) }
    var message by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(archivePath) {
        try {
            contents = repo.listArchive(archivePath)
        } catch (e: FussException) {
            error = e.userMessage()
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(File(archivePath).name, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Outlined.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            val c = contents
            if (error != null) {
                EmptyState(title = error!!, subtitle = "This archive format is not supported.")
                return@Column
            }
            if (c == null) {
                LoadingState()
                return@Column
            }
            Row(
                Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(Icons.Outlined.FolderZip, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
                Spacer(Modifier.width(10.dp))
                Column(Modifier.weight(1f)) {
                    Text(c.format.uppercase(), style = MaterialTheme.typography.labelLarge)
                    Text(
                        "${c.entries.size} entries · ${Format.size(c.totalBytes)}",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Button(
                    onClick = {
                        extracting = true
                        message = null
                    },
                    enabled = !extracting,
                ) { Text(if (extracting) "Extracting…" else "Extract") }
            }
            if (extracting) BusyBanner("Extracting archive")
            message?.let {
                Text(
                    it,
                    style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
                )
            }
            LazyColumn(Modifier.fillMaxSize()) {
                items(c.entries, key = { it.path }) { e ->
                    TwoLineRow(
                        primary = e.path,
                        secondary = if (e.isDir) "Folder" else "${Format.size(e.size)} · ${Format.size(e.compressedSize)} compressed",
                        leading = {
                            Icon(
                                if (e.isDir) Icons.Outlined.Folder else Icons.Outlined.FolderZip,
                                contentDescription = null,
                                modifier = Modifier.size(22.dp),
                                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        },
                    )
                }
            }
        }
    }

    LaunchedEffect(extracting) {
        if (extracting) {
            try {
                val destName = File(archivePath).name.substringBeforeLast('.')
                val dest = File(File(archivePath).parentFile, destName).absolutePath
                val r = repo.extractArchive(archivePath, dest)
                message = if (r.errors.isEmpty()) {
                    "Extracted ${r.files} files (${Format.size(r.bytes)}) to $destName"
                } else {
                    "Extracted ${r.files} files with ${r.errors.size} error(s): ${r.errors.first()}"
                }
                onExtracted(dest)
            } catch (e: FussException) {
                message = e.userMessage()
            }
            extracting = false
        }
    }
}
