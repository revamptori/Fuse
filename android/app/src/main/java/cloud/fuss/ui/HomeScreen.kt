package cloud.fuss.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
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
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.Folder
import androidx.compose.material.icons.outlined.History
import androidx.compose.material.icons.outlined.SdCard
import androidx.compose.material.icons.outlined.Star
import androidx.compose.material.icons.outlined.Storage
import androidx.compose.material3.AssistChip
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
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import cloud.fuss.app.RecentStore
import cloud.fuss.app.Root
import cloud.fuss.app.SafStore
import cloud.fuss.app.StorageRoots
import cloud.fuss.core.FsRepository
import cloud.fuss.core.StorageMount
import java.io.File

/**
 * Start screen: volumes, pinned folders, SAF grants and recent locations.
 * Everything here is a plain path that the browse screen can open.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HomeScreen(
    onOpen: (String) -> Unit,
    onAddSaf: () -> Unit,
    onSearch: () -> Unit,
) {
    val ctx = LocalContext.current
    val repo = remember { FsRepository() }
    val recentStore = remember { RecentStore(ctx) }
    val safStore = remember { SafStore(ctx) }
    var roots by remember { mutableStateOf<List<Root>>(emptyList()) }
    var mounts by remember { mutableStateOf<List<StorageMount>>(emptyList()) }
    var favorites by remember { mutableStateOf<List<String>>(emptyList()) }
    var recent by remember { mutableStateOf<List<String>>(emptyList()) }
    var saf by remember { mutableStateOf<List<SafStore.SafRoot>>(emptyList()) }
    var shortcuts by remember { mutableStateOf<List<Root>>(emptyList()) }
    var free by remember { mutableStateOf(0L) }
    var total by remember { mutableStateOf(0L) }

    fun reload() {
        roots = StorageRoots.discover(ctx)
        shortcuts = StorageRoots.shortcuts(ctx)
        favorites = recentStore.favorites()
        recent = recentStore.recent()
        saf = safStore.roots()
    }

    LaunchedEffect(Unit) {
        reload()
        mounts = repo.mounts()
        val shared = roots.firstOrNull { it.kind == cloud.fuss.app.RootKind.INTERNAL }?.path
        if (shared != null) {
            free = repo.freeSpace(shared)
            total = repo.totalSpace(shared)
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Fuss") },
                actions = {
                    IconButton(onClick = onSearch) { Icon(Icons.Outlined.History, contentDescription = null) }
                },
            )
        },
    ) { padding ->
        LazyColumn(Modifier.padding(padding).fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp)) {
            item {
                if (total > 0) {
                    Column(Modifier.padding(16.dp)) {
                        Text("Internal storage", style = MaterialTheme.typography.labelLarge)
                        Spacer(Modifier.height(4.dp))
                        androidx.compose.material3.LinearProgressIndicator(
                            progress = { ((total - free).toFloat() / total).coerceIn(0f, 1f) },
                            modifier = Modifier.fillMaxWidth(),
                        )
                        Spacer(Modifier.height(4.dp))
                        Text(
                            "${Format.size(free)} free of ${Format.size(total)}",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
            item { SectionHeader("Locations") }
            items(roots, key = { it.path }) { r ->
                LocationRow(
                    label = r.label,
                    path = r.path,
                    icon = when (r.kind) {
                        cloud.fuss.app.RootKind.SD_CARD -> Icons.Outlined.SdCard
                        cloud.fuss.app.RootKind.INTERNAL -> Icons.Outlined.Storage
                        else -> Icons.Outlined.Folder
                    },
                    onClick = { onOpen(r.path) },
                )
            }
            if (shortcuts.isNotEmpty()) {
                item { SectionHeader("Common folders") }
                item {
                    Column(Modifier.padding(horizontal = 16.dp)) {
                        shortcuts.chunked(3).forEach { row ->
                            Row(Modifier.fillMaxWidth().padding(vertical = 2.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                row.forEach { s ->
                                    AssistChip(onClick = { onOpen(s.path) }, label = { Text(s.label) })
                                }
                            }
                        }
                    }
                }
            }
            if (favorites.isNotEmpty()) {
                item { SectionHeader("Favorites") }
                items(favorites, key = { "fav-$it" }) { p ->
                    LocationRow(
                        label = File(p).name.ifEmpty { p },
                        path = p,
                        icon = Icons.Outlined.Star,
                        onClick = { onOpen(p) },
                        onRemove = {
                            recentStore.removeFavorite(p)
                            favorites = recentStore.favorites()
                        },
                    )
                }
            }
            item {
                Row(
                    Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text("Folder access (SAF)", style = MaterialTheme.typography.labelLarge, modifier = Modifier.weight(1f))
                    Button(onClick = { onAddSaf() }) {
                        Icon(Icons.Outlined.Add, contentDescription = null, modifier = Modifier.size(16.dp))
                        Spacer(Modifier.width(4.dp))
                        Text("Add")
                    }
                }
            }
            if (saf.isEmpty()) {
                item {
                    Text(
                        "Grant a folder to browse storage the system hides. Fuss asks only for the folder you pick.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
                    )
                }
            }
            items(saf, key = { "saf-${it.uri}" }) { r ->
                LocationRow(
                    label = r.label,
                    path = r.uri,
                    icon = Icons.Outlined.Folder,
                    onClick = { },
                    onRemove = {
                        safStore.remove(r)
                        saf = safStore.roots()
                    },
                    enabled = false,
                    subtitle = "SAF folder granted",
                )
            }
            if (recent.isNotEmpty()) {
                item { SectionHeader("Recent") }
                items(recent.take(10), key = { "recent-$it" }) { p ->
                    LocationRow(
                        label = File(p).name.ifEmpty { p },
                        path = p,
                        icon = Icons.Outlined.History,
                        onClick = { onOpen(p) },
                    )
                }
            }
            if (mounts.isNotEmpty()) {
                item { SectionHeader("Mounts") }
                items(mounts, key = { it.path }) { m ->
                    Text(
                        "${m.path}  ·  ${m.fsType}",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.padding(horizontal = 16.dp, vertical = 2.dp),
                    )
                }
            }
        }
    }
}

@Composable
private fun SectionHeader(text: String) {
    Column {
        Spacer(Modifier.height(8.dp))
        Text(
            text,
            style = MaterialTheme.typography.titleSmall,
            color = MaterialTheme.colorScheme.primary,
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 6.dp),
        )
        HorizontalDivider(Modifier.padding(horizontal = 16.dp))
    }
}

@Composable
private fun LocationRow(
    label: String,
    path: String,
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    onClick: () -> Unit,
    onRemove: (() -> Unit)? = null,
    enabled: Boolean = true,
    subtitle: String? = null,
) {
    TwoLineRow(
        primary = label,
        secondary = subtitle ?: path,
        leading = { Icon(icon, contentDescription = null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(22.dp)) },
        onClick = if (enabled) onClick else null,
    )
    if (onRemove != null) {
        // The remove affordance sits on its own small row to keep touch targets
        // large and the main row's tap area intact.
        Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 8.dp), horizontalArrangement = Arrangement.End) {
            IconButton(onClick = onRemove) {
                Icon(Icons.Outlined.Delete, contentDescription = "Remove", modifier = Modifier.size(18.dp))
            }
        }
    }
}
