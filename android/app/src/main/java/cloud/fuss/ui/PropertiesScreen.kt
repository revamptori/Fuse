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
import androidx.compose.material3.CircularProgressIndicator
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
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import cloud.fuss.app.RecentStore
import cloud.fuss.core.DirSizeResult
import cloud.fuss.core.FileProperties
import cloud.fuss.core.FsRepository
import cloud.fuss.core.FussException
import androidx.compose.ui.platform.LocalContext
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PropertiesScreen(
    path: String,
    onBack: () -> Unit,
    onOpenFolder: (String) -> Unit,
    onAddFavorite: (String) -> Unit,
) {
    val repo = remember { FsRepository() }
    val ctx = LocalContext.current
    var props by remember(path) { mutableStateOf<FileProperties?>(null) }
    var size by remember(path) { mutableStateOf<DirSizeResult?>(null) }
    var sizeLoading by remember(path) { mutableStateOf(false) }
    var error by remember(path) { mutableStateOf<String?>(null) }
    var favorite by remember(path) { mutableStateOf(RecentStore(ctx).isFavorite(path)) }
    var copied by remember { mutableStateOf(false) }

    LaunchedEffect(path) {
        try {
            props = repo.properties(path)
        } catch (e: FussException) {
            error = e.userMessage()
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
            if (error != null) {
                EmptyState(title = error!!)
                return@Column
            }
            if (p == null) {
                LoadingState()
                return@Column
            }
            Column(Modifier.fillMaxWidth().padding(16.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                FileThumb(
                    entry = cloud.fuss.core.FileEntry(p.name, p.isDir, p.isSymlink, true, 0, p.size, p.modified, p.mode),
                    dir = path.substringBeforeLast('/', "/"),
                    modifier = Modifier.height(96.dp).fillMaxWidth(),
                    corner = 12,
                )
            }
            Text(
                p.name,
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.padding(horizontal = 16.dp),
            )
            Spacer(Modifier.height(8.dp))
            HorizontalDivider()
            DetailRow("Path", p.path)
            DetailRow("Type", if (p.isDir) "Folder" else p.mime.ifEmpty { "File" })
            if (p.isDir) {
                DetailRow("Items", p.childCount.toString())
                if (sizeLoading) {
                    DetailRow("Size", "calculating…")
                } else {
                    DetailRow("Size", size?.let { Format.size(it.bytes) } ?: "Tap below to calculate")
                }
            } else {
                DetailRow("Size", Format.size(p.size))
                DetailRow("Size (bytes)", p.size.toString())
            }
            DetailRow("Modified", Format.dateTime(p.modified))
            DetailRow("Permissions", Format.permissions(p.mode))
            DetailRow("Owner", "${p.uid}:${p.gid}")
            if (p.isSymlink) DetailRow("Symlink to", p.symlinkTarget)
            DetailRow("Inode", p.inode.toString())
            DetailRow("Hard links", p.hardLinks.toString())
            HorizontalDivider()
            val actions = buildList {
                if (p.isDir) {
                    add("Calculate total size" to {
                        sizeLoading = true
                    })
                    add(if (favorite) "Remove from favorites" else "Add to favorites" to {
                        val store = RecentStore(ctx)
                        if (favorite) store.removeFavorite(path) else store.addFavorite(path)
                        favorite = !favorite
                        if (favorite) onAddFavorite(path)
                    })
                }
                add("Copy path" to {
                    val cm = ctx.getSystemService(android.content.ClipboardManager::class.java)
                    cm?.setPrimaryClip(android.content.ClipData.newPlainText("path", path))
                    copied = true
                })
            }
            actions.forEach { (label, action) ->
                androidx.compose.material3.TextButton(
                    onClick = {
                        action()
                        if (label == "Calculate total size") {
                            sizeLoading = true
                        }
                    },
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp),
                ) { Text(label) }
            }
            if (copied) {
                Text(
                    "Path copied",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = 16.dp),
                )
            }
            Spacer(Modifier.height(24.dp))
            if (sizeLoading) {
                BusyBanner("Calculating folder size")
            }
        }
    }

    // Size calculation runs outside the composition so scrolling stays smooth.
    LaunchedEffect(sizeLoading, path) {
        if (sizeLoading && props?.isDir == true) {
            size = try {
                withContext(Dispatchers.IO) { repo.dirSize(path) }
            } catch (e: Exception) {
                null
            }
            sizeLoading = false
        }
    }
}
