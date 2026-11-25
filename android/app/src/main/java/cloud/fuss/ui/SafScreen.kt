package cloud.fuss.ui

import androidx.compose.foundation.clickable
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
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.Download
import androidx.compose.material.icons.outlined.Folder
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
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import cloud.fuss.app.OpenWith
import cloud.fuss.app.SafBrowser
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File

/**
 * Browsing for a SAF-granted tree. Directories descend inside the tree; files
 * can be opened with the system handler, copied into the app's Download folder
 * (so the Rust engine can then operate on them) or deleted when the grant
 * allows it.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SafScreen(
    treeUri: String,
    treeLabel: String,
    onBack: () -> Unit,
    onCopied: (String) -> Unit,
) {
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    var stack by remember { mutableStateOf(listOf(treeUri to treeLabel)) }
    var nodes by remember { mutableStateOf<List<SafBrowser.SafNode>>(emptyList()) }
    var loading by remember { mutableStateOf(true) }
    var message by remember { mutableStateOf<String?>(null) }
    val (curUri, curLabel) = stack.last()

    LaunchedEffect(curUri) {
        loading = true
        nodes = withContext(Dispatchers.IO) { runCatching { SafBrowser.children(ctx, curUri) }.getOrDefault(emptyList()) }
        loading = false
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(curLabel, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                navigationIcon = {
                    IconButton(onClick = { if (stack.size > 1) stack = stack.dropLast(1) else onBack() }) {
                        Icon(Icons.AutoMirrored.Outlined.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            message?.let {
                Text(
                    it,
                    style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                )
            }
            when {
                loading -> LoadingState()
                nodes.isEmpty() -> EmptyState(
                    title = "Nothing here",
                    subtitle = "The grant may have been revoked, or the folder is empty.",
                )
                else -> LazyColumn(Modifier.fillMaxSize()) {
                    items(nodes, key = { it.uri }) { n ->
                        Row(
                            Modifier.fillMaxWidth()
                                .clickable {
                                    if (n.isDir) {
                                        stack = stack + (n.uri to n.name)
                                    } else {
                                        val uri = android.net.Uri.parse(n.uri)
                                        val mime = SafBrowser.document(ctx, n.uri)?.type ?: "*/*"
                                        runCatching {
                                            ctx.startActivity(
                                                android.content.Intent(android.content.Intent.ACTION_VIEW).apply {
                                                    setDataAndType(uri, mime)
                                                    addFlags(android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION)
                                                    addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK)
                                                }
                                            )
                                        }.onFailure { message = "No app can open this file" }
                                    }
                                }
                                .padding(horizontal = 16.dp, vertical = 10.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Icon(
                                if (n.isDir) Icons.Outlined.Folder else FileIcons.icon(SafBrowser.asEntry(n).kind),
                                contentDescription = null,
                                tint = if (n.isDir) MaterialTheme.colorScheme.primary else FileIcons.tint(SafBrowser.asEntry(n).kind),
                                modifier = Modifier.size(22.dp),
                            )
                            Spacer(Modifier.width(12.dp))
                            Column(Modifier.weight(1f)) {
                                Text(n.name, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                Text(
                                    if (n.isDir) "Folder" else Format.size(n.size),
                                    style = MaterialTheme.typography.labelSmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                            }
                            if (!n.isDir) {
                                IconButton(onClick = {
                                    scope.launch {
                                        // SAF has no filesystem path, so a tree
                                        // can only join the engine by being
                                        // streamed somewhere real. App storage
                                        // is the one writable location that
                                        // needs no permission and accepts any
                                        // file type, so imports land there and
                                        // the user is taken to them.
                                        val dest = File(ctx.filesDir, "imports")
                                        val copied = withContext(Dispatchers.IO) { SafBrowser.copyTo(ctx, n.uri, dest) }
                                        if (copied == null) {
                                            message = "Copy failed: ${n.name}"
                                        } else {
                                            message = "Copied ${copied.name} into Fuss storage"
                                            onCopied(copied.absolutePath)
                                        }
                                    }
                                }) {
                                    Icon(Icons.Outlined.Download, contentDescription = "Copy into Fuss", modifier = Modifier.size(20.dp))
                                }
                                IconButton(onClick = {
                                    scope.launch {
                                        val ok = withContext(Dispatchers.IO) { SafBrowser.delete(ctx, n.uri) }
                                        message = if (ok) "Deleted ${n.name}" else "Could not delete ${n.name}"
                                        nodes = withContext(Dispatchers.IO) { SafBrowser.children(ctx, curUri) }
                                    }
                                }) {
                                    Icon(Icons.Outlined.Delete, contentDescription = "Delete", modifier = Modifier.size(20.dp))
                                }
                            }
                        }
                    }
                }
            }
            Spacer(Modifier.height(8.dp))
        }
    }
}
