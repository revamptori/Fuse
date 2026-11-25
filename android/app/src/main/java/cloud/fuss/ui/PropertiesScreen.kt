package cloud.fuss.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import cloud.fuss.app.RecentStore
import cloud.fuss.core.DirSizeResult
import cloud.fuss.core.FileEntry
import cloud.fuss.core.FileProperties
import cloud.fuss.core.FsRepository
import cloud.fuss.core.FussException
import java.io.File

/**
 * Properties for one path. The folder size is the only expensive fact here, so
 * it is opt-in via a button and runs with a cancel token; leaving the screen
 * cancels the scan through the composition's coroutine scope.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PropertiesScreen(
    path: String,
    onBack: () -> Unit,
) {
    val repo = remember { FsRepository() }
    val ctx = LocalContext.current
    var props by remember(path) { mutableStateOf<FileProperties?>(null) }
    var dirSize by remember(path) { mutableStateOf<DirSizeResult?>(null) }
    var sizeLoading by remember(path) { mutableStateOf(false) }
    var error by remember(path) { mutableStateOf<String?>(null) }
    var favorite by remember(path) { mutableStateOf(RecentStore(ctx).isFavorite(path)) }

    LaunchedEffect(path) {
        try {
            props = repo.properties(path)
        } catch (e: FussException) {
            error = e.userMessage()
        }
    }

    LaunchedEffect(sizeLoading) {
        if (sizeLoading) {
            dirSize = runCatching { repo.dirSize(path) }.getOrNull()
            sizeLoading = false
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Properties") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Outlined.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState())) {
            val p = props
            when {
                error != null -> EmptyState(title = error!!)
                p == null -> LoadingState()
                else -> {
                    Column(
                        Modifier.fillMaxWidth().padding(vertical = 20.dp),
                        horizontalAlignment = Alignment.CenterHorizontally,
                    ) {
                        FileThumb(
                            entry = FileEntry(p.name, p.isDir, p.isSymlink, true, if (p.isDir) FileEntry.KIND_DIRECTORY else FileEntry.KIND_OTHER, p.size, p.modified, p.mode),
                            dir = path.substringBeforeLast('/', "/"),
                            modifier = Modifier.size(80.dp),
                            corner = 16,
                        )
                        Spacer(Modifier.height(10.dp))
                        Text(p.name, style = MaterialTheme.typography.titleMedium)
                    }
                    HorizontalDivider()
                    DetailRow("Path", p.path)
                    DetailRow("Type", if (p.isDir) "Folder" else p.mime.ifEmpty { "File" })
                    if (p.isDir) {
                        DetailRow("Items", p.childCount.toString())
                        DetailRow(
                            "Size",
                            when {
                                sizeLoading -> "calculating…"
                                dirSize != null -> Format.size(dirSize!!.bytes) + " (" + dirSize!!.files +
                                    " files, " + dirSize!!.dirs + " folders)"
                                else -> "not calculated"
                            },
                        )
                    } else {
                        DetailRow("Size", "${Format.size(p.size)} (${p.size} bytes)")
                    }
                    DetailRow("Modified", Format.dateTime(p.modified))
                    DetailRow("Permissions", Format.permissions(p.mode))
                    DetailRow("Owner", "uid ${p.uid}, gid ${p.gid}")
                    DetailRow("Hard links", p.hardLinks.toString())
                    DetailRow("Inode", p.inode.toString())
                    if (p.isSymlink) DetailRow("Link target", p.symlinkTarget)
                    HorizontalDivider()
                    if (p.isDir) {
                        TextButton(
                            onClick = { if (!sizeLoading) sizeLoading = true },
                            modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp),
                        ) { Text(if (dirSize == null) "Calculate folder size" else "Recalculate size") }
                    }
                    TextButton(
                        onClick = {
                            val store = RecentStore(ctx)
                            if (favorite) store.removeFavorite(path) else store.addFavorite(path)
                            favorite = !favorite
                        },
                        modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp),
                    ) { Text(if (favorite) "Remove from favorites" else "Add to favorites") }
                    TextButton(
                        onClick = {
                            val cm = ctx.getSystemService(android.content.ClipboardManager::class.java)
                            cm?.setPrimaryClip(android.content.ClipData.newPlainText("path", path))
                        },
                        modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp),
                    ) { Text("Copy path") }
                    Spacer(Modifier.height(24.dp))
                }
            }
        }
    }
}
