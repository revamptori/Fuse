package cloud.fuss.ui

import android.net.Uri
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.viewmodel.compose.viewModel
import cloud.fuss.app.BrowseViewModel
import cloud.fuss.app.OpenWith
import cloud.fuss.core.FileEntry

/**
 * Screen stack. A hand-written back stack (not Navigation-Compose) keeps the
 * dependency set small and the behaviour obvious: one enum plus one path.
 */
private sealed interface Screen {
    data object Home : Screen
    data class Browse(val path: String) : Screen
    data class Properties(val path: String) : Screen
    data class Archive(val path: String) : Screen
    data class Storage(val path: String) : Screen
    data class Search(val root: String) : Screen
    data object Settings : Screen
}

@Composable
fun FussNav(darkOverride: Boolean?, dynamic: Boolean) {
    val ctx = LocalContext.current
    val vm: BrowseViewModel = viewModel()
    val ui by vm.ui.collectAsState()
    var stack by remember { mutableStateOf(listOf<Screen>(Screen.Home)) }
    val current = stack.last()
    fun push(s: Screen) { stack = stack + s }
    fun pop() { if (stack.size > 1) stack = stack.dropLast(1) }

    val safLauncher = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri: Uri? ->
        if (uri != null) {
            vm.attachSaf(uri)
        }
    }

    FussTheme(dark = darkOverride, dynamicColor = dynamic) {
        BackHandler(enabled = current !is Screen.Home || ui.state.selectMode) {
            when {
                ui.state.selectMode -> vm.clearSelection()
                else -> pop()
            }
        }

        when (val s = current) {
            is Screen.Home -> HomeScreen(
                onOpen = { path ->
                    vm.open(path)
                    push(Screen.Browse(path))
                },
                onAddSaf = { safLauncher.launch(null) },
                onSearch = {
                    vm.open(if (ui.state.path.isEmpty()) "/storage/emulated/0" else ui.state.path, record = false)
                    push(Screen.Search(ui.state.path.ifEmpty { "/storage/emulated/0" }))
                },
            )
            is Screen.Browse -> BrowseScreen(
                vm = vm,
                ui = ui,
                onOpenFolder = { path ->
                    vm.open(path)
                    push(Screen.Browse(path))
                },
                onOpenFile = { path, entry ->
                    val archiveFile = entry.kind == FileEntry.KIND_ARCHIVE || vm.isArchiveSync(path)
                    when {
                        archiveFile -> push(Screen.Archive(path))
                        !OpenWith.open(ctx, path, entry) -> {
                            vm.open(ui.state.path, record = false)
                            // Fall through: keep the user in place; the snackbar
                            // explains that no viewer exists.
                            android.widget.Toast.makeText(ctx, "No app can open this file", android.widget.Toast.LENGTH_SHORT).show()
                        }
                    }
                },
                onProperties = { path -> push(Screen.Properties(path)) },
                onOpenArchive = { path -> push(Screen.Archive(path)) },
                onShare = { path ->
                    if (!OpenWith.share(ctx, listOf(path))) {
                        android.widget.Toast.makeText(ctx, "Nothing to share with", android.widget.Toast.LENGTH_SHORT).show()
                    }
                },
            )
            is Screen.Properties -> PropertiesScreen(
                path = s.path,
                onBack = { pop() },
                onOpenFolder = { p -> vm.open(p); push(Screen.Browse(p)) },
                onAddFavorite = { },
            )
            is Screen.Archive -> ArchiveScreen(
                archivePath = s.path,
                onBack = { pop() },
                onExtracted = { dest -> vm.open(dest) },
            )
            is Screen.Storage -> StorageScreen(
                root = s.path,
                onBack = { pop() },
                onOpen = { p ->
                    vm.open(p)
                    push(Screen.Browse(p))
                },
            )
            is Screen.Search -> SearchScreen(
                root = s.root,
                onBack = { pop() },
                onOpenPath = { path ->
                    vm.open(path)
                    push(Screen.Browse(path))
                },
            )
            is Screen.Settings -> SettingsScreen(onBack = { pop() })
        }
    }
}
