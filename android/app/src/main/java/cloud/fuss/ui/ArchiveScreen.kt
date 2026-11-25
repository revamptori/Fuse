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
import androidx.compose.material.icons.outlined.FolderOpen
import androidx.compose.material.icons.outlined.FolderZip
import androidx.compose.material.icons.outlined.InsertDriveFile
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
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
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import cloud.fuss.core.ArchiveContents
import cloud.fuss.core.FsRepository
import cloud.fuss.core.FussException
import kotlinx.coroutines.launch
import java.io.File

/**
 * Archive viewer. The listing comes from the engine's central-directory/tar
 * parser, so a 4 GB ZIP opens instantly without extraction. Extraction and
 * creation both stream through the engine with a cancel token.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ArchiveScreen(
    archivePath: String,
    onBack: () -> Unit,
    onExtracted: (String) -> Unit,
) {
    val repo = remember { FsRepository() }
    val scope = rememberCoroutineScope()
    var contents by remember(archivePath) { mutableStateOf<ArchiveContents?>(null) }
    var error by remember(archivePath) { mutableStateOf<String?>(null) }
    var extracting by remember { mutableStateOf(false) }
    var message by remember { mutableStateOf<String?>(null) }
    var query by remember { mutableStateOf("") }

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
            when {
                error != null -> EmptyState(
                    title = error!!,
                    subtitle = "Supported formats: ZIP, TAR, TAR.GZ/TGZ, GZIP, BZIP2-deflate variants are not read.",
                )
                contents == null -> LoadingState()
                else -> {
                    val c = contents!!
                    Row(
                        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Icon(Icons.Outlined.FolderZip, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
                        Spacer(Modifier.width(10.dp))
                        Column(Modifier.weight(1f)) {
                            Text(c.format.uppercase(), style = MaterialTheme.typography.labelLarge)
                            Text(
                                "${c.entries.size} entries · ${Format.size(c.totalBytes)} " +
                                    "(${(if (c.totalBytes > 0) 100L * c.totalCompressed / c.totalBytes else 0)}% stored)",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        Button(
                            enabled = !extracting,
                            onClick = {
                                extracting = true
                                message = null
                                scope.launch {
                                    try {
                                        val destName = File(archivePath).name
                                            .removeSuffix(".gz").removeSuffix(".tar")
                                            .substringBeforeLast('.', File(archivePath).name)
                                        val dest = File(File(archivePath).parentFile, destName).absolutePath
                                        val r = repo.extractArchive(archivePath, dest)
                                        message = if (r.errors.isEmpty()) {
                                            "Extracted ${r.files} files (${Format.size(r.bytes)}) into ${File(dest).name}"
                                        } else {
                                            "${r.files} extracted, ${r.errors.size} failed: ${r.errors.first()}"
                                        }
                                        onExtracted(dest)
                                    } catch (e: FussException) {
                                        message = e.userMessage()
                                    } finally {
                                        extracting = false
                                    }
                                }
                            },
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
                    HorizontalDivider()
                    LazyColumn(Modifier.fillMaxSize()) {
                        items(c.entries, key = { it.path }) { e ->
                            Row(
                                Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Icon(
                                    when {
                                        e.isDir -> Icons.Outlined.Folder
                                        e.path.endsWith("/") -> Icons.Outlined.FolderOpen
                                        else -> Icons.Outlined.InsertDriveFile
                                    },
                                    contentDescription = null,
                                    modifier = Modifier.size(20.dp),
                                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                                Spacer(Modifier.width(10.dp))
                                Column(Modifier.weight(1f)) {
                                    Text(e.path, style = MaterialTheme.typography.bodyMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
                                    if (!e.isDir) {
                                        Text(
                                            "${Format.size(e.size)}" +
                                                (if (e.compressedSize > 0) " · ${Format.size(e.compressedSize)} compressed" else ""),
                                            style = MaterialTheme.typography.labelSmall,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                        )
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
