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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.Folder
import androidx.compose.material.icons.outlined.History
import androidx.compose.material.icons.outlined.SdCard
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material.icons.outlined.Star
import androidx.compose.material.icons.outlined.Storage
import androidx.compose.material3.AssistChip
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SuggestionChip
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
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
import cloud.fuss.app.RootKind
import cloud.fuss.app.SafStore
import cloud.fuss.app.StorageRoots
import cloud.fuss.core.FsRepository
import cloud.fuss.core.StorageMount
import java.io.File

/**
 * Start screen: volumes, pinned folders, SAF grants and recent locations.
 * Everything here resolves to a path the browse screen can open; SAF trees open
 * the platform-side browser instead, because content:// has no path.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HomeScreen(
    onOpen: (String) -> Unit,
    onOpenSaf: (SafStore.SafRoot) -> Unit,
    onAddSaf: () -> Unit,
    onSettings: () -> Unit,
) {
    val ctx = LocalContext.current
    val repo = remember { FsRepository() }
    val recentStore = remember { RecentStore(ctx) }
    val safStore = remember { SafStore(ctx) }
    var reload by remember { mutableIntStateOf(0) }
    var mounts by remember { mutableStateOf<List<StorageMount>>(emptyList()) }
    var free by remember { mutableStateOf(0L) }
    var total by remember { mutableStateOf(0L) }

    val roots = remember(reload) { StorageRoots.discover(ctx) }
    val shortcuts = remember(reload) { StorageRoots.shortcuts(ctx) }
    val favorites = remember(reload) { recentStore.favorites() }
    val recent = remember(reload) { recentStore.recent() }
    val saf = remember(reload) { safStore.roots() }

    LaunchedEffect(Unit) {
        mounts = runCatching { repo.mounts() }.getOrDefault(emptyList())
        roots.firstOrNull { it.kind == RootKind.INTERNAL }?.let { r ->
            free = runCatching { repo.freeSpace(r.path) }.getOrDefault(0L)
            total = runCatching { repo.totalSpace(r.path) }.getOrDefault(0L)
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Fuss") },
                actions = {
                    IconButton(onClick = onSettings) { Icon(Icons.Outlined.Settings, contentDescription = "Settings") }
                },
            )
        },
    ) { padding ->
        LazyColumn(
            Modifier.padding(padding).fillMaxSize(),
            contentPadding = PaddingValues(bottom = 32.dp),
        ) {
            item {
                if (total > 0) {
                    Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) {
                        LinearProgressIndicator(
                            progress = { ((total - free).toFloat() / total).coerceIn(0f, 1f) },
                            modifier = Modifier.fillMaxWidth().height(6.dp),
                        )
                        Spacer(Modifier.height(6.dp))
                        Text(
                            "${Format.size(free)} free of ${Format.size(total)}",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
            item { SectionHeader("Locations") }
            items(roots, key = { "root-${it.path}" }) { r ->
                LocationRow(
                    label = r.label,
                    path = r.path,
                    icon = when (r.kind) {
                        RootKind.SD_CARD -> Icons.Outlined.SdCard
                        RootKind.INTERNAL -> Icons.Outlined.Storage
                        else -> Icons.Outlined.Folder
                    },
                    onClick = { onOpen(r.path) },
                )
            }
            if (shortcuts.isNotEmpty()) {
                item { SectionHeader("Common folders") }
                item {
                    Column(Modifier.padding(horizontal = 16.dp, vertical = 4.dp)) {
                        shortcuts.chunked(3).forEach { row ->
                            Row(
                                Modifier.fillMaxWidth().padding(vertical = 2.dp),
                                horizontalArrangement = Arrangement.spacedBy(6.dp),
                            ) {
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
                    )
                }
            }
            item {
                Row(
                    Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text("Folder access (SAF)", style = MaterialTheme.typography.titleSmall, modifier = Modifier.weight(1f))
                    SuggestionChip(onClick = onAddSaf, label = { Text("Add folder") }, icon = {
                        Icon(Icons.Outlined.Add, contentDescription = null, modifier = Modifier.size(16.dp))
                    })
                }
            }
            if (saf.isEmpty()) {
                item {
                    Text(
                        "Android hides parts of shared storage from every app. Grant a folder here and Fuss " +
                            "can list and import from it: no broad storage permission needed.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(horizontal = 16.dp, vertical = 2.dp),
                    )
                }
            }
            items(saf, key = { "saf-${it.uri}" }) { r ->
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        TwoLineRow(
                            primary = r.label,
                            secondary = "Granted tree · content://",
                            leading = {
                                Icon(
                                    Icons.Outlined.Folder,
                                    contentDescription = null,
                                    tint = MaterialTheme.colorScheme.primary,
                                    modifier = Modifier.size(22.dp),
                                )
                            },
                            onClick = { onOpenSaf(r) },
                        )
                    }
                    IconButton(onClick = {
                        safStore.remove(r)
                        reload++
                    }) {
                        Icon(Icons.Outlined.Delete, contentDescription = "Remove grant", modifier = Modifier.size(18.dp))
                    }
                }
            }
            if (recent.isNotEmpty()) {
                item { SectionHeader("Recent") }
                items(recent.take(12), key = { "recent-$it" }) { p ->
                    LocationRow(
                        label = File(p).name.ifEmpty { p },
                        path = p,
                        icon = Icons.Outlined.History,
                        onClick = { onOpen(p) },
                    )
                }
            }
            if (mounts.isNotEmpty()) {
                item { SectionHeader("System mounts") }
                items(mounts, key = { "mnt-${it.path}" }) { m ->
                    Text(
                        "${m.path} · ${m.fsType}",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.padding(horizontal = 16.dp, vertical = 1.dp),
                    )
                }
            }
        }
    }
}

@Composable
private fun SectionHeader(text: String) {
    Column {
        Spacer(Modifier.height(10.dp))
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
) {
    TwoLineRow(
        primary = label,
        secondary = path,
        leading = {
            Icon(icon, contentDescription = null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(22.dp))
        },
        onClick = onClick,
    )
}
