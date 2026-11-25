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
import androidx.compose.material.icons.outlined.Folder
import androidx.compose.material.icons.outlined.InsertDriveFile
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
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
import cloud.fuss.core.ChildDirSize
import cloud.fuss.core.DuplicateResults
import cloud.fuss.core.FileEntry
import cloud.fuss.core.FsRepository
import cloud.fuss.core.FussException
import cloud.fuss.core.LargeFile
import cloud.fuss.core.TypeStat
import java.io.File

private const val TAB_OVERVIEW = 0
private const val TAB_LARGE = 1
private const val TAB_EMPTY = 2
private const val TAB_TYPES = 3
private const val TAB_DUPLICATES = 4

private val TAB_LABELS = arrayOf("Size", "Large", "Empty", "Types", "Dupes")

/**
 * Storage analysis for one root. Each tab triggers exactly one engine scan with
 * its own cancel token; leaving the screen cancels the running scan.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun StorageScreen(root: String, onBack: () -> Unit, onOpen: (String) -> Unit) {
    val repo = remember { FsRepository() }
    var tab by remember { mutableStateOf(TAB_OVERVIEW) }
    var busy by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf("") }

    var childSizes by remember { mutableStateOf(emptyList<ChildDirSize>()) }
    var totalBytes by remember { mutableStateOf(0L) }
    var largest by remember { mutableStateOf(emptyList<LargeFile>()) }
    var empties by remember { mutableStateOf(emptyList<String>()) }
    var types by remember { mutableStateOf(emptyList<TypeStat>()) }
    var dups by remember { mutableStateOf<DuplicateResults?>(null) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        "Storage: " + File(root).name,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Outlined.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
    ) { padding ->
        Column(
            Modifier
                .padding(padding)
                .fillMaxSize(),
        ) {
            SingleChoiceSegmentedButtonRow(
                Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 8.dp),
            ) {
                for (i in TAB_LABELS.indices) {
                    SegmentedButton(
                        selected = tab == i,
                        onClick = { tab = i },
                        shape = SegmentedButtonDefaults.itemShape(i, TAB_LABELS.size),
                    ) {
                        Text(TAB_LABELS[i], style = MaterialTheme.typography.labelSmall, maxLines = 1)
                    }
                }
            }
            Spacer(Modifier.height(8.dp))
            if (busy) {
                BusyBanner("Analysing…")
            }
            if (error.isNotEmpty()) {
                EmptyState(title = error)
            } else if (tab == TAB_OVERVIEW) {
                OverviewList(childSizes, totalBytes, onOpen)
            } else if (tab == TAB_LARGE) {
                LargeList(largest, onOpen)
            } else if (tab == TAB_EMPTY) {
                EmptyList(empties)
            } else if (tab == TAB_TYPES) {
                TypeList(types)
            } else {
                DuplicateList(dups, busy)
            }
        }
    }

    LaunchedEffect(root, tab) {
        busy = true
        error = ""
        try {
            if (tab == TAB_OVERVIEW) {
                val sizes = repo.childDirSizes(root)
                childSizes = sizes
                var sum = 0L
                for (d in sizes) sum += d.bytes
                totalBytes = sum
            } else if (tab == TAB_LARGE) {
                largest = repo.largestFiles(root, 150)
            } else if (tab == TAB_EMPTY) {
                empties = repo.emptyDirs(root, 500)
            } else if (tab == TAB_TYPES) {
                types = repo.typeStats(root)
            } else {
                dups = repo.duplicates(root)
            }
        } catch (e: FussException) {
            if (!e.isCancellation) error = e.userMessage()
        } catch (e: Exception) {
            // A cancelled scan surfaces as an ordinary exception; only report it
            // when it is not a coroutine cancellation.
            if (e !is kotlinx.coroutines.CancellationException) {
                error = e.message ?: "Analysis failed"
            }
        } finally {
            busy = false
        }
    }
}

@Composable
private fun OverviewList(items: List<ChildDirSize>, total: Long, onOpen: (String) -> Unit) {
    if (items.isEmpty()) {
        EmptyState(title = "Nothing to summarise", subtitle = "No sub-folders here.")
        return
    }
    LazyColumn(Modifier.fillMaxSize()) {
        item {
            Column(Modifier.padding(16.dp)) {
                Text(Format.size(total) + " in folders", style = MaterialTheme.typography.titleMedium)
                Text(
                    items.size.toString() + " top-level folders, largest first",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        items(items) { d ->
            val frac: Float = if (total > 0L) d.bytes.toFloat() / total.toFloat() else 0f
            Column(
                Modifier
                    .fillMaxWidth()
                    .clickable { onOpen(d.path) }
                    .padding(horizontal = 16.dp, vertical = 6.dp),
            ) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(
                        Icons.Outlined.Folder,
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.size(20.dp),
                    )
                    Spacer(Modifier.width(10.dp))
                    Text(
                        File(d.path).name,
                        Modifier.weight(1f),
                        style = MaterialTheme.typography.bodyLarge,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text(Format.size(d.bytes), style = MaterialTheme.typography.bodyMedium)
                }
                Spacer(Modifier.height(4.dp))
                LinearProgressIndicator(
                    progress = { frac },
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(3.dp),
                )
                Text(
                    d.files.toString() + " files · " + d.dirs.toString() + " folders",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(top = 2.dp),
                )
            }
        }
    }
}

@Composable
private fun LargeList(items: List<LargeFile>, onOpen: (String) -> Unit) {
    if (items.isEmpty()) {
        EmptyState(title = "No files found")
        return
    }
    LazyColumn(Modifier.fillMaxSize()) {
        items(items) { f ->
            val dir = f.path.substringBeforeLast('/')
            TwoLineRow(
                primary = File(f.path).name,
                secondary = Format.size(f.size) + " · " + dir,
                leading = {
                    Icon(Icons.Outlined.InsertDriveFile, contentDescription = null, modifier = Modifier.size(22.dp))
                },
                onClick = { onOpen(dir) },
            )
        }
    }
}

@Composable
private fun EmptyList(items: List<String>) {
    if (items.isEmpty()) {
        EmptyState(title = "No empty folders", subtitle = "Nothing to clean up.")
        return
    }
    LazyColumn(Modifier.fillMaxSize()) {
        items(items) { p ->
            TwoLineRow(
                primary = File(p).name,
                secondary = p,
                leading = {
                    Icon(Icons.Outlined.Folder, contentDescription = null, modifier = Modifier.size(22.dp))
                },
            )
        }
    }
}

private fun kindName(kind: Int): String {
    return when (kind) {
        FileEntry.KIND_IMAGE -> "Images"
        FileEntry.KIND_VIDEO -> "Videos"
        FileEntry.KIND_AUDIO -> "Audio"
        FileEntry.KIND_TEXT -> "Text"
        FileEntry.KIND_PDF -> "PDF"
        FileEntry.KIND_DOCUMENT -> "Documents"
        FileEntry.KIND_SPREADSHEET -> "Spreadsheets"
        FileEntry.KIND_PRESENTATION -> "Presentations"
        FileEntry.KIND_ARCHIVE -> "Archives"
        FileEntry.KIND_APK -> "Apps"
        FileEntry.KIND_CODE -> "Code"
        FileEntry.KIND_FONT -> "Fonts"
        else -> "Other"
    }
}

@Composable
private fun TypeList(items: List<TypeStat>) {
    if (items.isEmpty()) {
        EmptyState(title = "No files scanned")
        return
    }
    LazyColumn(Modifier.fillMaxSize()) {
        items(items) { t ->
            Row(
                Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp, vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(
                    FileIcons.icon(t.kind),
                    contentDescription = null,
                    tint = FileIcons.tint(t.kind),
                    modifier = Modifier.size(20.dp),
                )
                Spacer(Modifier.width(12.dp))
                Column(Modifier.weight(1f)) {
                    Text(kindName(t.kind), style = MaterialTheme.typography.bodyLarge)
                    Text(
                        t.count.toString() + " files",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Text(Format.size(t.bytes), style = MaterialTheme.typography.bodyMedium)
            }
        }
    }
}

@Composable
private fun DuplicateList(dups: DuplicateResults?, busy: Boolean) {
    if (dups == null) {
        if (busy) LoadingState() else EmptyState(title = "No duplicate scan yet")
        return
    }
    if (dups.groups.isEmpty()) {
        EmptyState(title = "No duplicates found", subtitle = "Nothing to reclaim.")
        return
    }
    LazyColumn(Modifier.fillMaxSize()) {
        item {
            Column(Modifier.padding(16.dp)) {
                Text(Format.size(dups.totalWasted) + " reclaimable", style = MaterialTheme.typography.titleMedium)
                Text(
                    dups.groups.size.toString() + " groups of byte-identical files (size, then SHA-256)",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        items(dups.groups) { g ->
            Column(
                Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp, vertical = 6.dp),
            ) {
                Text(File(g.paths[0]).name, style = MaterialTheme.typography.bodyLarge)
                Text(
                    g.paths.size.toString() + " copies · " + Format.size(g.size) + " each",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                for (path in g.paths) {
                    Text(
                        path,
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }
    }
}
