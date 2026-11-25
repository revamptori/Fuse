package cloud.fuss.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.ContentCopy
import androidx.compose.material.icons.outlined.ContentCut
import androidx.compose.material.icons.outlined.ContentPaste
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.DoneAll
import androidx.compose.material.icons.outlined.Edit
import androidx.compose.material.icons.outlined.Folder
import androidx.compose.material.icons.outlined.FolderZip
import androidx.compose.material.icons.outlined.Home
import androidx.compose.material.icons.outlined.PieChart
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material.icons.outlined.MoreVert
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material.icons.outlined.SelectAll
import androidx.compose.material.icons.outlined.Share
import androidx.compose.material.icons.outlined.Sort
import androidx.compose.material.icons.outlined.Visibility
import androidx.compose.material.icons.outlined.VisibilityOff
import androidx.compose.material3.AssistChip
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import cloud.fuss.app.BrowseUi
import cloud.fuss.app.BrowseViewModel
import cloud.fuss.app.SortChoice
import cloud.fuss.core.FileEntry
import cloud.fuss.core.SortSpec
import coil.compose.AsyncImage
import coil.request.ImageRequest
import java.io.File

@OptIn(ExperimentalMaterial3Api::class, ExperimentalFoundationApi::class)
@Composable
fun BrowseScreen(
    vm: BrowseViewModel,
    ui: BrowseUi,
    onOpenFolder: (String) -> Unit,
    onOpenFile: (String, FileEntry) -> Unit,
    onProperties: (String) -> Unit,
    onOpenArchive: (String) -> Unit,
    onShare: (String) -> Unit,
    onNewArchive: () -> Unit = {},
    onAnalyse: () -> Unit = {},
    onHome: () -> Unit = {},
) {
    val ctx = LocalContext.current
    val snackbar = remember { SnackbarHostState() }
    var menuOpen by remember { mutableStateOf(false) }
    var sortOpen by remember { mutableStateOf(false) }
    var layoutGrid by remember { mutableStateOf(ctx.getSharedPreferences("fuss", 0).getBoolean("grid", false)) }
    val s = ui.state

    LaunchedEffect(s.message) {
        val msg = s.message ?: return@LaunchedEffect
        snackbar.showSnackbar(msg)
        vm.consumeMessage()
    }

    Scaffold(
        snackbarHost = { SnackbarHost(snackbar) },
        topBar = {
            if (s.selectMode) {
                SelectionBar(
                    count = s.selection.size,
                    onClose = { vm.clearSelection() },
                    onSelectAll = { vm.selectAll() },
                    onCopy = { vm.copySelection() },
                    onCut = { vm.cutSelection() },
                    onDelete = { vm.requestDelete(s.selection.map { vm.pathOf(it) }) },
                )
            } else {
                TopAppBar(
                    title = {
                        Text(
                            text = s.crumbs.lastOrNull()?.label ?: "Fuss",
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    },
                    navigationIcon = {
                        if (s.parentPath != null && s.parentPath != "/") {
                            IconButton(onClick = { vm.navigateUp() }) {
                                Icon(Icons.AutoMirrored.Outlined.ArrowBack, contentDescription = "Up")
                            }
                        }
                    },
                    actions = {
                        if (ui.pendingPaste != null) {
                            IconButton(onClick = { vm.pasteHere() }) {
                                Icon(Icons.Outlined.ContentPaste, contentDescription = "Paste here")
                            }
                        }
                        IconButton(onClick = { sortOpen = true }) {
                            Icon(Icons.Outlined.Sort, contentDescription = "Sort")
                        }
                        DropdownMenu(expanded = sortOpen, onDismissRequest = { sortOpen = false }) {
                            SortItem("Name", SortSpec.NAME, s.sortKey, s.ascending, vm, sortOpen) { sortOpen = false }
                            SortItem("Size", SortSpec.SIZE, s.sortKey, s.ascending, vm, sortOpen) { sortOpen = false }
                            SortItem("Type", SortSpec.TYPE, s.sortKey, s.ascending, vm, sortOpen) { sortOpen = false }
                            SortItem("Date", SortSpec.MODIFIED, s.sortKey, s.ascending, vm, sortOpen) { sortOpen = false }
                            HorizontalDivider()
                            DropdownMenuItem(
                                text = { Text(if (s.showHidden) "Hide hidden files" else "Show hidden files") },
                                leadingIcon = {
                                    Icon(
                                        if (s.showHidden) Icons.Outlined.VisibilityOff else Icons.Outlined.Visibility,
                                        contentDescription = null,
                                    )
                                },
                                onClick = { sortOpen = false; vm.toggleHidden() },
                            )
                            DropdownMenuItem(
                                text = { Text(if (layoutGrid) "List view" else "Grid view") },
                                onClick = {
                                    layoutGrid = !layoutGrid
                                    ctx.getSharedPreferences("fuss", 0).edit().putBoolean("grid", layoutGrid).apply()
                                    sortOpen = false
                                },
                            )
                            DropdownMenuItem(
                                text = { Text("Refresh") },
                                leadingIcon = { Icon(Icons.Outlined.Refresh, contentDescription = null) },
                                onClick = { sortOpen = false; vm.refresh() },
                            )
                        }
                        IconButton(onClick = { menuOpen = true }) {
                            Icon(Icons.Outlined.MoreVert, contentDescription = "More")
                        }
                        DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
                            DropdownMenuItem(
                                text = { Text("New folder") },
                                leadingIcon = { Icon(Icons.Outlined.Add, contentDescription = null) },
                                onClick = { menuOpen = false; vm.showDialogCreateFolder() },
                            )
                            DropdownMenuItem(
                                text = { Text("New file") },
                                onClick = { menuOpen = false; vm.showDialogCreateFile() },
                            )
                            DropdownMenuItem(
                                text = { Text("Compress to ZIP") },
                                leadingIcon = { Icon(Icons.Outlined.FolderZip, contentDescription = null) },
                                onClick = { menuOpen = false; onNewArchive() },
                            )
                            DropdownMenuItem(
                                text = { Text("Storage analysis") },
                                leadingIcon = { Icon(Icons.Outlined.PieChart, contentDescription = null) },
                                onClick = { menuOpen = false; onAnalyse() },
                            )
                            DropdownMenuItem(
                                text = { Text("Home") },
                                leadingIcon = { Icon(Icons.Outlined.Home, contentDescription = null) },
                                onClick = { menuOpen = false; onHome() },
                            )
                            HorizontalDivider()
                            DropdownMenuItem(
                                text = { Text("Select all") },
                                leadingIcon = { Icon(Icons.Outlined.SelectAll, contentDescription = null) },
                                onClick = { menuOpen = false; vm.selectAll() },
                            )
                        }
                    },
                    colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.surface),
                )
            }
        },
        bottomBar = {
            if (!s.selectMode && s.totalBytes > 0) {
                StorageBar(free = s.freeBytes, total = s.totalBytes)
            }
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            if (s.crumbs.size > 1) {
                Breadcrumbs(s.crumbs) { c -> vm.open(c.path) }
            }
            if (s.truncated) {
                // The engine caps one listing call; rather than pretend the
                // folder ends there, say so. Sorting is applied per call, so a
                // naive page-two would break the order the user is reading.
                Text(
                    text = "Showing the first ${s.entries.size} items — this folder has more. " +
                        "Search here, or use the system picker for the whole set.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                )
            }
            Box(Modifier.fillMaxSize()) {
                when {
                    s.loading -> LoadingState()
                    s.error != null -> EmptyState(
                        title = s.error,
                        subtitle = when (s.errorCode) {
                            "permission_denied" -> "Use \"Add folder access\" on the home screen to open it through the system picker."
                            "not_found" -> "It may have been moved or deleted."
                            else -> null
                        },
                    )
                    s.entries.isEmpty() -> EmptyState(
                        title = "Empty folder",
                        subtitle = if (s.readOnly) "This location is read-only." else "Create a folder or paste files here.",
                    )
                    layoutGrid -> EntryGrid(
                        entries = s.entries,
                        selection = s.selection,
                        selectMode = s.selectMode,
                        dir = s.path,
                        onOpen = { e ->
                            val p = vm.pathOf(e.name)
                            if (e.isDir) onOpenFolder(p) else onOpenFile(p, e)
                        },
                        onLongPress = { e -> if (!s.selectMode) vm.startSelection(e.name) else vm.toggleSelect(e.name) },
                        onToggle = { e -> vm.toggleSelect(e.name) },
                    )
                    else -> EntryList(
                        entries = s.entries,
                        selection = s.selection,
                        selectMode = s.selectMode,
                        dir = s.path,
                        onOpen = { e ->
                            val p = vm.pathOf(e.name)
                            if (e.isDir) onOpenFolder(p) else onOpenFile(p, e)
                        },
                        onLongPress = { e -> if (!s.selectMode) vm.startSelection(e.name) else vm.toggleSelect(e.name) },
                        onToggle = { e -> vm.toggleSelect(e.name) },
                        onMenu = { e, action ->
                            val p = vm.pathOf(e.name)
                            when (action) {
                                RowAction.OPEN -> if (e.isDir) onOpenFolder(p) else onOpenFile(p, e)
                                RowAction.PROPERTIES -> onProperties(p)
                                RowAction.RENAME -> vm.showDialogRename(e.name)
                                RowAction.DELETE -> vm.requestDelete(listOf(p))
                                RowAction.SHARE -> onShare(p)
                                RowAction.COPY -> { vm.startSelection(e.name); vm.copySelection() }
                                RowAction.CUT -> { vm.startSelection(e.name); vm.cutSelection() }
                                RowAction.ARCHIVE -> onOpenArchive(p)
                            }
                        },
                    )
                }
                if (s.loading && s.entries.isNotEmpty()) {
                    Box(Modifier.fillMaxWidth().padding(8.dp), contentAlignment = Alignment.TopCenter) {
                        CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
                    }
                }
            }
        }
    }

    ui.dialog?.let { dlg ->
        NameDialog(dlg, onDismiss = { vm.dismissDialog() }, onSubmit = { vm.submitDialog(it) })
    }
    ui.confirm?.let { c ->
        ConfirmDialog(
            title = c.title,
            text = c.text,
            confirmLabel = c.confirmLabel,
            onDismiss = { vm.dismissConfirm() },
            onConfirm = { c.onConfirm() },
        )
    }
}

@Composable
private fun SortItem(
    label: String,
    key: Int,
    currentKey: Int,
    currentAsc: Boolean,
    vm: BrowseViewModel,
    open: Boolean,
    close: () -> Unit,
) {
    val active = key == currentKey
    val arrow = if (active) (if (currentAsc) " ↑" else " ↓") else ""
    DropdownMenuItem(
        text = { Text("$label$arrow") },
        onClick = {
            val asc = if (active) !currentAsc else true
            vm.setSort(SortSpec(key, asc))
            close()
        },
    )
}

@Composable
private fun Breadcrumbs(crumbs: List<cloud.fuss.app.Crumb>, onJump: (cloud.fuss.app.Crumb) -> Unit) {
    val state = rememberLazyListState()
    LaunchedEffect(crumbs.size) {
        if (crumbs.isNotEmpty()) state.scrollToItem(crumbs.lastIndex)
    }
    LazyRow(
        state = state,
        modifier = Modifier.fillMaxWidth(),
        contentPadding = PaddingValues(horizontal = 12.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        items(crumbs) { c ->
            AssistChip(
                onClick = { onJump(c) },
                label = { Text(c.label, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                modifier = Modifier.padding(end = 6.dp),
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SelectionBar(
    count: Int,
    onClose: () -> Unit,
    onSelectAll: () -> Unit,
    onCopy: () -> Unit,
    onCut: () -> Unit,
    onDelete: () -> Unit,
) {
    TopAppBar(
        title = { Text("$count selected") },
        navigationIcon = {
            IconButton(onClick = onClose) { Icon(Icons.Outlined.Close, contentDescription = "Cancel selection") }
        },
        actions = {
            IconButton(onClick = onSelectAll) { Icon(Icons.Outlined.SelectAll, contentDescription = "Select all") }
            IconButton(onClick = onCopy) { Icon(Icons.Outlined.ContentCopy, contentDescription = "Copy") }
            IconButton(onClick = onCut) { Icon(Icons.Outlined.ContentCut, contentDescription = "Cut") }
            IconButton(onClick = onDelete) { Icon(Icons.Outlined.Delete, contentDescription = "Delete") }
        },
    )
}

@Composable
private fun StorageBar(free: Long, total: Long) {
    val used = (total - free).coerceAtLeast(0)
    val fraction = if (total > 0) (used.toFloat() / total.toFloat()).coerceIn(0f, 1f) else 0f
    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp)) {
        Box(
            Modifier
                .fillMaxWidth()
                .height(3.dp)
                .clip(RoundedCornerShape(2.dp))
                .background(MaterialTheme.colorScheme.surfaceVariant),
        ) {
            Box(
                Modifier
                    .fillMaxWidth(fraction)
                    .fillMaxHeight()
                    .background(if (fraction > 0.92f) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.primary),
            )
        }
        Spacer(Modifier.height(4.dp))
        Text(
            "${Format.size(free)} free of ${Format.size(total)}",
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

enum class RowAction { OPEN, PROPERTIES, RENAME, DELETE, SHARE, COPY, CUT, ARCHIVE }

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun EntryList(
    entries: List<FileEntry>,
    selection: Set<String>,
    selectMode: Boolean,
    dir: String,
    onOpen: (FileEntry) -> Unit,
    onLongPress: (FileEntry) -> Unit,
    onToggle: (FileEntry) -> Unit,
    onMenu: (FileEntry, RowAction) -> Unit,
) {
    val state = rememberLazyListState()
    LazyColumn(state = state, modifier = Modifier.fillMaxSize()) {
        items(entries, key = { it.name }) { e ->
            val selected = selection.contains(e.name)
            EntryRow(
                entry = e,
                dir = dir,
                selected = selected,
                selectMode = selectMode,
                onClick = { if (selectMode) onToggle(e) else onOpen(e) },
                onLongClick = { onLongPress(e) },
                onMenu = { action -> onMenu(e, action) },
            )
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun EntryRow(
    entry: FileEntry,
    dir: String,
    selected: Boolean,
    selectMode: Boolean,
    onClick: () -> Unit,
    onLongClick: () -> Unit,
    onMenu: (RowAction) -> Unit,
) {
    var menu by remember { mutableStateOf(false) }
    val bg = if (selected) MaterialTheme.colorScheme.primaryContainer else Color.Transparent
    Row(
        Modifier
            .fillMaxWidth()
            .background(bg)
            .combinedClickable(onClick = onClick, onLongClick = onLongClick)
            .padding(start = 16.dp, end = 4.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        FileThumb(entry, dir, Modifier.size(36.dp))
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(entry.name, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                secondaryLine(entry),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
            )
        }
        if (selectMode) {
            Icon(
                if (selected) Icons.Outlined.DoneAll else Icons.Outlined.Folder,
                contentDescription = null,
                tint = if (selected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outline,
                modifier = Modifier.padding(8.dp).size(18.dp),
            )
        } else {
            Box {
                IconButton(onClick = { menu = true }) {
                    Icon(Icons.Outlined.MoreVert, contentDescription = "Actions", tint = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                RowMenu(expanded = menu, entry = entry, onDismiss = { menu = false }, onAction = { menu = false; onMenu(it) })
            }
        }
    }
}

@Composable
private fun RowMenu(expanded: Boolean, entry: FileEntry, onDismiss: () -> Unit, onAction: (RowAction) -> Unit) {
    DropdownMenu(expanded = expanded, onDismissRequest = onDismiss) {
        MenuItem("Open", Icons.Outlined.Folder) { onAction(RowAction.OPEN) }
        if (entry.kind == FileEntry.KIND_ARCHIVE) {
            MenuItem("Open as archive", Icons.Outlined.Folder) { onAction(RowAction.ARCHIVE) }
        }
        MenuItem("Copy", Icons.Outlined.ContentCopy) { onAction(RowAction.COPY) }
        MenuItem("Cut", Icons.Outlined.ContentCut) { onAction(RowAction.CUT) }
        MenuItem("Rename", Icons.Outlined.Edit) { onAction(RowAction.RENAME) }
        MenuItem("Share", Icons.Outlined.Share) { onAction(RowAction.SHARE) }
        MenuItem("Properties", Icons.Outlined.Info) { onAction(RowAction.PROPERTIES) }
        MenuItem("Delete", Icons.Outlined.Delete) { onAction(RowAction.DELETE) }
    }
}

@Composable
private fun MenuItem(label: String, icon: ImageVector, onClick: () -> Unit) {
    DropdownMenuItem(
        text = { Text(label) },
        leadingIcon = { Icon(icon, contentDescription = null) },
        onClick = onClick,
    )
}

private fun secondaryLine(e: FileEntry): String {
    val date = Format.relativeDate(e.modified)
    return if (e.isDir) date else "${Format.size(e.size)} · $date"
}

/** Small preview for images, icon for everything else. */
@Composable
fun FileThumb(entry: FileEntry, dir: String, modifier: Modifier = Modifier, corner: Int = 8) {
    val ctx = LocalContext.current
    val shape = RoundedCornerShape(corner.dp)
    if (entry.kind == FileEntry.KIND_IMAGE) {
        val file = File(if (dir.endsWith("/")) dir + entry.name else "$dir/${entry.name}")
        AsyncImage(
            model = ImageRequest.Builder(ctx)
                .data(file)
                .crossfade(false)
                .build(),
            contentDescription = null,
            contentScale = ContentScale.Crop,
            modifier = modifier.clip(shape),
        )
    } else {
        Box(modifier, contentAlignment = Alignment.Center) {
            Icon(
                FileIcons.icon(entry.kind),
                contentDescription = null,
                tint = FileIcons.tint(entry.kind),
                modifier = Modifier.size(26.dp),
            )
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun EntryGrid(
    entries: List<FileEntry>,
    selection: Set<String>,
    selectMode: Boolean,
    dir: String,
    onOpen: (FileEntry) -> Unit,
    onLongPress: (FileEntry) -> Unit,
    onToggle: (FileEntry) -> Unit,
) {
    LazyVerticalGrid(
        columns = GridCells.Adaptive(96.dp),
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(12.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        items(entries, key = { it.name }) { e ->
            val selected = selection.contains(e.name)
            Column(
                Modifier
                    .clip(RoundedCornerShape(10.dp))
                    .background(if (selected) MaterialTheme.colorScheme.primaryContainer else Color.Transparent)
                    .combinedClickable(
                        onClick = { if (selectMode) onToggle(e) else onOpen(e) },
                        onLongClick = { onLongPress(e) },
                    )
                    .padding(6.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                if (e.kind == FileEntry.KIND_IMAGE) {
                    FileThumb(e, dir, Modifier.size(72.dp), corner = 8)
                } else {
                    Box(Modifier.size(72.dp), contentAlignment = Alignment.Center) {
                        Icon(
                            FileIcons.icon(e.kind),
                            contentDescription = null,
                            tint = FileIcons.tint(e.kind),
                            modifier = Modifier.size(38.dp),
                        )
                    }
                }
                Spacer(Modifier.height(6.dp))
                Text(
                    e.name,
                    style = MaterialTheme.typography.labelMedium,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                )
            }
        }
    }
}
