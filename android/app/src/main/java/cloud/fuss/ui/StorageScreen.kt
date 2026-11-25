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
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.Folder
import androidx.compose.material.icons.outlined.InsertDriveFile
import androidx.compose.material.icons.outlined.Layers
import androidx.compose.material.icons.outlined.Storage
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
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

private enum class Tab(val label: String) { OVERVIEW("Overview"), LARGE("Largest"), EMPTY("Empty"), TYPES("Types"), DUPLICATES("Duplicates") }

/**
 * Storage analysis for one root: folder breakdown, largest files, empty
 * folders, type statistics and duplicate detection. Every scan runs as its own
 * coroutine with a cancel handle, so leaving the screen stops the work.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun StorageScreen(root: String, onBack: () -> Unit, onOpen: (String) -> Unit) {
    val repo = remember { FsRepository() }
    var tab by remember { mutableStateOf(Tab.OVERVIEW) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }

    var childSizes by remember(root) { mutableStateOf<List<ChildDirSize>>(emptyList()) }
    var largest by remember(root) { mutableStateOf<List<LargeFile>>(emptyList()) }
    var empties by remember(root) { mutableStateOf<List<String>>(emptyList()) }
    var types by remember(root) { mutableStateOf<List<TypeStat>>(emptyList()) }
    var dups by remember(root) { mutableStateOf<DuplicateResults?>(null) }
    var totalBytes by remember(root) { mutableStateOf(0L) }

    fun load(t: Tab) {
        tab = t
        busy = true
        error = null
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Storage: ${File(root).name.ifEmpty { root }}", maxLines = 1, overflow = TextOverflow.Ellipsis) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Outlined.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth().padding(horizontal = 12.dp)) {
                Tab.entries.forEachIndexed { i, t ->
                    SegmentedButton(
                        selected = tab == t,
                        onClick = { load(t) },
                        shape = SegmentedButtonDefaults.itemShape(i, Tab.entries.size),
                    ) { Text(t.label, style = MaterialTheme.typography.labelSmall) }
                }
            }
            Spacer(Modifier.height(8.dp))
            if (busy) BusyBanner("Analysing…")
            if (error != null) EmptyState(title = error!!)
            when (tab) {
                Tab.OVERVIEW -> OverviewList(childSizes, totalBytes, onOpen)
                Tab.LARGE -> FileList(largest)
                Tab.EMPTY -> EmptyList(empties)
                Tab.TYPES -> TypeList(types)
                Tab.DUPLICATES -> DuplicateList(dups)
            }
        }
    }

    LaunchedEffect(root, tab) {
        try {
            when (tab) {
                Tab.OVERVIEW -> {
                    childSizes = repo.childDirSizes(root)
                    totalBytes = childSizes.sumOf { it.bytes }
                }
                Tab.LARGE -> largest = repo.largestFiles(root, 150)
                Tab.EMPTY -> empties = repo.emptyDirs(root, 500)
                Tab.TYPES -> types = repo.typeStats(root)
                Tab.DUPLICATES -> dups = repo.duplicates(root)
            }
        } catch (e: FussException) {
            error = e.userMessage()
        } catch (e: CancellationExceptionHolder.None) {
            // unreachable; kept for exhaustive when
        } catch (e: Exception) {
            error = e.message
        }
        busy = false
    }
}

private object CancellationExceptionHolder { class None : Exception() }

@Composable
private fun OverviewList(items: List<ChildDirSize>, total: Long, onOpen: (String) -> Unit) {
    if (items.isEmpty()) {
        EmptyState(title = "Nothing to summarise", subtitle = "No sub-folders here.")
        return
    }
    LazyColumn(Modifier.fillMaxSize()) {
        item {
            Column(Modifier.padding(16.dp)) {
                Text("${Format.size(total)} in folders", style = MaterialTheme.typography.titleMedium)
                Text(
                    "${items.size} top-level folders",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        items(items, key = { it.path }) { d ->
            Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.Outlined.Folder, contentDescription = null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(20.dp))
                    Spacer(Modifier.width(10.dp))
                    Text(File(d.path).name, Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge, maxLines = 1)
                    Text(Format.size(d.bytes), style = MaterialTheme.typography.bodyMedium)
                }
                Spacer(Modifier.height(4.dp))
                val frac = if (total > 0) d.bytes.toFloat() / total else 0f
                LinearProgressIndicator(progress = { frac }, modifier = Modifier.fillMaxWidth().height(3.dp))
                Text(
                    "${d.files} files · ${d.dirs} folders",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(top = 3.dp),
                )
            }
        }
    }
}

@Composable
private fun FileList(items: List<LargeFile>) {
    if (items.isEmpty()) {
        EmptyState(title = "No files found")
        return
    }
    LazyColumn(Modifier.fillMaxSize()) {
        items(items, key = { it.path }) { f ->
            TwoLineRow(
                primary = File(f.path).name,
                secondary = "${Format.size(f.size)} · ${f.path.substringBeforeLast('/')}",
                leading = { Icon(Icons.Outlined.InsertDriveFile, contentDescription = null, modifier = Modifier.size(22.dp)) },
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
                leading = { Icon(Icons.Outlined.Folder, contentDescription = null, modifier = Modifier.size(22.dp)) },
            )
        }
    }
}

@Composable
private fun TypeList(items: List<TypeStat>) {
    val total = items.sumOf { it.bytes }
    LazyColumn(Modifier.fillMaxSize()) {
        items(items, key = { it.kind }) { t ->
            val name = when (t.kind) {
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
            Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                Icon(FileIcons.icon(t.kind), contentDescription = null, tint = FileIcons.tint(t.kind), modifier = Modifier.size(20.dp))
                Spacer(Modifier.width(12.dp))
                Column(Modifier.weight(1f)) {
                    Text(name, style = MaterialTheme.typography.bodyLarge)
                    Text("${t.count} files", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                Text(Format.size(t.bytes), style = MaterialTheme.typography.bodyMedium)
            }
        }
    }
}

@Composable
private fun DuplicateList(dups: DuplicateResults?) {
    if (dups == null) {
        EmptyState(title = "Scanning for duplicates…", subtitle = "Files are grouped by size, then by content hash.")
        return
    }
    if (dups.groups.isEmpty()) {
        EmptyState(title = "No duplicates found", subtitle = "Nothing to reclaim.")
        return
    }
    LazyColumn(Modifier.fillMaxSize()) {
        item {
            Column(Modifier.padding(16.dp)) {
                Text("${Format.size(dups.totalWasted)} reclaimable", style = MaterialTheme.typography.titleMedium)
                Text(
                    "${dups.groups.size} groups of identical files",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        items(dups.groups, key = { it.paths.first() }) { g ->
            Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp)) {
                Text(File(g.paths.first()).name, style = MaterialTheme.typography.bodyLarge)
                Text(
                    "${g.paths.size} copies · ${Format.size(g.size)} each",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                g.paths.forEach { p ->
                    Text(p, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
        }
    }
}
