package cloud.fuss.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.net.Uri
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.viewmodel.compose.viewModel
import cloud.fuss.app.BrowseViewModel
import cloud.fuss.app.OpenWith
import cloud.fuss.app.SafStore
import cloud.fuss.core.FileEntry
import java.io.File

/**
 * Screen stack. A hand-written back stack (not Navigation-Compose) keeps the
 * dependency set small and the behaviour obvious: a sealed `Screen` plus paths.
 * The Browse screen is the only screen backed by the shared [BrowseViewModel];
 * everything else is transient and stateless on recomposition.
 */
private sealed interface Screen {
    data object Home : Screen
    data class Browse(val path: String) : Screen
    data class Properties(val path: String) : Screen
    data class Archive(val path: String) : Screen
    data class Compress(val dir: String, val selection: List<String>) : Screen
    data class Storage(val path: String) : Screen
    data class Search(val root: String) : Screen
    data class Saf(val uri: String, val label: String) : Screen
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
        if (uri != null) vm.attachSaf(uri)
    }

    FussTheme(dark = darkOverride, dynamicColor = dynamic) {
        BackHandler(enabled = current !is Screen.Home || ui.state.selectMode) {
            if (ui.state.selectMode) vm.clearSelection() else pop()
        }

        when (val s = current) {
            is Screen.Home -> HomeScreen(
                onOpen = { path ->
                    vm.open(path)
                    push(Screen.Browse(path))
                },
                onOpenSaf = { root -> push(Screen.Saf(root.uri, root.label)) },
                onAddSaf = { safLauncher.launch(null) },
                onSettings = { push(Screen.Settings) },
            )
            is Screen.Browse -> BrowseScreen(
                vm = vm,
                ui = ui,
                onOpenFolder = { path ->
                    vm.open(path)
                    push(Screen.Browse(path))
                },
                onOpenFile = { path, entry: FileEntry ->
                    when {
                        entry.kind == FileEntry.KIND_ARCHIVE || vm.isArchiveSync(path) -> push(Screen.Archive(path))
                        !OpenWith.open(ctx, path, entry) ->
                            android.widget.Toast.makeText(ctx, "No app can open this file", android.widget.Toast.LENGTH_SHORT).show()
                    }
                },
                onProperties = { path -> push(Screen.Properties(path)) },
                onOpenArchive = { path -> push(Screen.Archive(path)) },
                onShare = { path ->
                    if (!OpenWith.share(ctx, listOf(path))) {
                        android.widget.Toast.makeText(ctx, "Nothing can share this", android.widget.Toast.LENGTH_SHORT).show()
                    }
                },
                onNewArchive = {
                    val st = ui.state
                    val sel = st.selection.map { vm.pathOf(it) }
                    vm.clearSelection()
                    push(Screen.Compress(st.path, sel))
                },
                onAnalyse = { push(Screen.Storage(ui.state.path)) },
                onHome = {
                    while (stack.size > 1) stack = stack.dropLast(1)
                },
            )
            is Screen.Properties -> PropertiesScreen(path = s.path, onBack = { pop() })
            is Screen.Archive -> ArchiveScreen(
                archivePath = s.path,
                onBack = { pop() },
                onExtracted = { dest ->
                    vm.open(dest, record = false)
                },
            )
            is Screen.Compress -> CompressScreen(
                directory = s.dir,
                selection = s.selection,
                onBack = { pop() },
                onDone = { created ->
                    val clip = ctx.getSystemService(ClipboardManager::class.java)
                    clip?.setPrimaryClip(ClipData.newPlainText("fuss", created))
                    vm.refresh()
                },
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
            is Screen.Saf -> SafScreen(
                treeUri = s.uri,
                treeLabel = s.label,
                onBack = { pop() },
                onCopied = { importedPath ->
                    // Jump to the folder the import landed in so the user can see
                    // it immediately, instead of being told to go hunting.
                    File(importedPath).parent?.let { dir ->
                        vm.open(dir)
                        push(Screen.Browse(dir))
                    }
                },
            )
            is Screen.Settings -> SettingsScreen(onBack = { pop() })
        }
    }
}
