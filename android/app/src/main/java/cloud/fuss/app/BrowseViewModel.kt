package cloud.fuss.app

import android.app.Application
import android.net.Uri
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import cloud.fuss.core.CollisionMode
import cloud.fuss.core.FileEntry
import cloud.fuss.core.FsRepository
import cloud.fuss.core.FussException
import cloud.fuss.core.FussNative
import cloud.fuss.core.OpResult
import cloud.fuss.core.SortSpec
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File

/** One breadcrumb hop. */
data class Crumb(val label: String, val path: String)

data class UiState(
    val path: String = "",
    val crumbs: List<Crumb> = emptyList(),
    val entries: List<FileEntry> = emptyList(),
    val loading: Boolean = true,
    val error: String? = null,
    val errorCode: String? = null,
    val sortKey: Int = SortSpec.NAME,
    val ascending: Boolean = true,
    val showHidden: Boolean = false,
    val selection: Set<String> = emptySet(),
    val selectMode: Boolean = false,
    val message: String? = null,
    val readOnly: Boolean = false,
    val freeBytes: Long = 0,
    val totalBytes: Long = 0,
    val parentPath: String? = null,
    val ticks: Long = 0,
    /** The directory holds more entries than one listing returns. */
    val truncated: Boolean = false,
)

data class DialogState(
    val title: String,
    val label: String,
    val initial: String = "",
    val action: DialogAction,
    val description: String? = null,
)

enum class DialogAction { CREATE_FILE, CREATE_FOLDER, RENAME }

data class ConfirmState(
    val title: String,
    val text: String,
    val confirmLabel: String = "Delete",
    val onConfirm: () -> Unit,
)

enum class SortChoice { NAME_ASC, NAME_DESC, SIZE_DESC, SIZE_ASC, DATE_DESC, DATE_ASC, TYPE_ASC, TYPE_DESC }

data class BrowseUi(
    val state: UiState = UiState(),
    val dialog: DialogState? = null,
    val confirm: ConfirmState? = null,
    val pendingPaste: List<String>? = null,
    val pendingPasteWasCut: Boolean = false,
)

/**
 * All browsing state and actions for the file list.
 *
 * Threading contract: every filesystem call goes through [FsRepository] which
 * switches to IO; this class only touches immutable state on the main thread.
 * Directory loads are cancellable and stale loads are dropped by comparing the
 * request path, so fast navigation cannot render the wrong folder.
 */
class BrowseViewModel(app: Application) : AndroidViewModel(app) {

    private val repo = FsRepository()
    private val prefs = app.getSharedPreferences("fuss", 0)
    private val _ui = MutableStateFlow(BrowseUi())
    val ui: StateFlow<BrowseUi> = _ui.asStateFlow()

    private var loadJob: Job? = null
    private var lastPath: String? = null

    init {
        val last = prefs.getString("last_path", null)
        val start = when {
            last != null && File(last).isDirectory -> last
            File("/storage/emulated/0").isDirectory -> "/storage/emulated/0"
            else -> getApplication<Application>().filesDir.absolutePath
        }
        open(start, record = false)
    }

    /** Native engine version; surfaced in Settings > About. */
    fun engineVersion(): String = runCatching { FussNative.version() }.getOrDefault("unavailable")

    fun selfTest(): String = runCatching { FussNative.selfTest() }.getOrDefault("unavailable")

    // ------------------------------------------------------------------ load

    /**
     * True when the native engine is loaded. Probed once, lazily: touching
     * [FussNative] runs its `System.loadLibrary` initialiser, which throws
     * [UnsatisfiedLinkError] when the .so is missing for this ABI.
     */
    private val engineAvailable: Boolean by lazy {
        try {
            FussNative.version()
            true
        } catch (t: Throwable) {
            _ui.update {
                it.copy(
                    state = it.state.copy(
                        loading = false,
                        error = "The file engine could not be loaded on this device.",
                        errorCode = "engine_missing",
                    ),
                )
            }
            false
        }
    }

    fun open(path: String, record: Boolean = true) {
        loadJob?.cancel()
        _ui.update {
            it.copy(
                state = it.state.copy(path = path, loading = true, error = null, errorCode = null, selection = emptySet(), selectMode = false),
            )
        }
        loadJob = viewModelScope.launch {
            val s = _ui.value.state
            // UnsatisfiedLinkError is an Error, not an Exception, so a missing or
            // ABI-mismatched libfuss_core.so would escape every catch below and
            // kill the process on the first frame — the same "opens and closes"
            // symptom as a bad manifest entry. Report it in the UI instead.
            if (!engineAvailable) return@launch
            try {
                val page = repo.list(path, s.showHidden, SortSpec(s.sortKey, s.ascending))
                if (_ui.value.state.path != path) return@launch
                val free = repo.freeSpace(path)
                val total = repo.totalSpace(path)
                val readOnly = !canWrite(path)
                val parent = File(path).parent
                _ui.update {
                    it.copy(
                        state = it.state.copy(
                            path = path,
                            crumbs = buildCrumbs(path),
                            entries = page.entries,
                            loading = false,
                            error = null,
                            errorCode = null,
                            freeBytes = free,
                            totalBytes = total,
                            parentPath = parent,
                            readOnly = readOnly,
                            truncated = page.hasMore,
                        ),
                    )
                }
                lastPath = path
                if (record) {
                    prefs.edit().putString("last_path", path).apply()
                }
                // Recents are a small file, but this runs on the very first
                // frame of a cold start; keep every disk touch off the main
                // thread, as the rest of this class does.
                withContext(Dispatchers.IO) { RecentStore(getApplication()).add(path) }
            } catch (e: FussException) {
                if (_ui.value.state.path != path) return@launch
                _ui.update {
                    it.copy(
                        state = it.state.copy(
                            loading = false,
                            error = e.userMessage(),
                            errorCode = e.code,
                            entries = emptyList(),
                            path = path,
                            crumbs = buildCrumbs(path),
                        ),
                    )
                }
            } catch (e: Exception) {
                if (_ui.value.state.path != path) return@launch
                _ui.update {
                    it.copy(state = it.state.copy(loading = false, error = e.message ?: "Could not open folder", entries = emptyList()))
                }
            }
        }
    }

    fun refresh() {
        open(_ui.value.state.path, record = false)
    }

    private suspend fun canWrite(path: String): Boolean = withContext(Dispatchers.IO) {
        File(path).canWrite()
    }

    fun navigateUp(): Boolean {
        val parent = _ui.value.state.parentPath ?: return false
        open(parent)
        return true
    }

    /**
     * Breadcrumbs for the path, with friendly labels for the standard
     * locations: "/storage/emulated/0" reads "Internal storage", and each
     * removable volume keeps its description from the StorageManager.
     */
    private fun buildCrumbs(path: String): List<Crumb> {
        val parts = ArrayDeque<String>()
        var cur = File(path)
        while (cur.absolutePath != "/") {
            parts.addFirst(cur.name)
            cur = cur.parentFile ?: break
        }
        val out = ArrayList<Crumb>(parts.size + 2)
        val internal = "/storage/emulated/0"
        if (path == internal) {
            out += Crumb("Internal storage", internal)
            return out
        }
        out += Crumb("/", "/")
        val acc = StringBuilder()
        var internalLabeled = false
        for ((i, p) in parts.withIndex()) {
            acc.append('/').append(p)
            val full = acc.toString()
            val label = when {
                full == internal -> {
                    internalLabeled = true
                    "Internal storage"
                }
                // Collapse the emulated-storage plumbing into one hop.
                full == "/storage/emulated" -> "Internal storage"
                full == "/storage" -> "storage"
                full == "/mnt" -> "mnt"
                else -> p
            }
            // Skip the "0" that follows "/storage/emulated"; the previous crumb
            // already points at shared storage.
            if (i > 0 && p == "0" && parts.getOrNull(i - 1) == "emulated") continue
            if (full == "/storage/emulated") continue
            out += Crumb(label, full)
        }
        if (!internalLabeled && out.isEmpty()) out += Crumb("/", "/")
        // A path inside /storage/emulated/0 still needs the friendly root.
        if (path.startsWith("$internal/") ) {
            val idx = out.indexOfFirst { it.path == internal }
            if (idx < 0) {
                // Replace "/storage/emulated" crumb (or "storage") with a single
                // "Internal storage" crumb pointing at the real root, then drop
                // redundant ancestors between.
                val trimmed = out.filter { c -> c.path.startsWith(internal) || c.path == "/" }
                val below = trimmed.filter { it.path != "/" && it.path.startsWith(internal) }
                return buildList {
                    add(Crumb("/", "/"))
                    add(Crumb("Internal storage", internal))
                    below.forEach { if (it.path != internal) add(it) }
                }.ifEmpty { out }
            }
        }
        return out
    }

    // ------------------------------------------------------------------ sort

    fun setSort(spec: SortSpec) {
        _ui.update { it.copy(state = it.state.copy(sortKey = spec.key, ascending = spec.ascending)) }
        resortLocal()
    }

    fun toggleHidden() {
        _ui.update { it.copy(state = it.state.copy(showHidden = !it.state.showHidden)) }
        open(_ui.value.state.path, record = false)
    }

    private fun resortLocal() {
        // Sorting is cheap and already deterministic in Rust; re-listing is the
        // simplest correct path and keeps one implementation of the order.
        refresh()
    }

    fun cycleSortByName(): SortSpec {
        val s = _ui.value.state
        return if (s.sortKey == SortSpec.NAME) SortSpec(SortSpec.NAME, !s.ascending) else SortSpec(SortSpec.NAME, true)
    }

    // -------------------------------------------------------------- selection

    fun toggleSelect(name: String) {
        _ui.update {
            val sel = if (it.state.selection.contains(name)) it.state.selection - name else it.state.selection + name
            it.copy(state = it.state.copy(selection = sel, selectMode = sel.isNotEmpty() || it.state.selectMode))
        }
    }

    fun startSelection(name: String) {
        _ui.update { it.copy(state = it.state.copy(selectMode = true, selection = it.state.selection + name)) }
    }

    fun clearSelection() {
        _ui.update { it.copy(state = it.state.copy(selection = emptySet(), selectMode = false)) }
    }

    fun selectAll() {
        _ui.update {
            val all = it.state.entries.map { e -> e.name }.toSet()
            val newSel = if (it.state.selection.size == all.size) emptySet() else all
            it.copy(state = it.state.copy(selection = newSel, selectMode = newSel.isNotEmpty()))
        }
    }

    private fun selectedPaths(): List<String> {
        val s = _ui.value.state
        return s.selection.map { name -> join(s.path, name) }
    }

    private fun join(dir: String, name: String): String =
        if (dir.endsWith("/")) dir + name else "$dir/$name"

    private fun parentOf(path: String): String = File(path).parent ?: "/"

    private fun baseName(path: String): String = File(path).name

    // ------------------------------------------------------------------ ops

    private suspend fun guard(block: suspend () -> Unit) {
        try {
            block()
        } catch (e: FussException) {
            snack(e.userMessage())
        } catch (e: Exception) {
            snack(e.message ?: "Operation failed")
        }
    }

    private fun snack(text: String) {
        _ui.update { it.copy(state = it.state.copy(message = text)) }
    }

    fun consumeMessage() {
        _ui.update { it.copy(state = it.state.copy(message = null)) }
    }

    fun showDialogCreateFile() {
        _ui.update { it.copy(dialog = DialogState("New file", "File name", "", DialogAction.CREATE_FILE)) }
    }

    fun showDialogCreateFolder() {
        _ui.update { it.copy(dialog = DialogState("New folder", "Folder name", "", DialogAction.CREATE_FOLDER)) }
    }

    fun showDialogRename(name: String) {
        _ui.update { it.copy(dialog = DialogState("Rename", "New name", name, DialogAction.RENAME, description = name)) }
    }

    fun dismissDialog() {
        _ui.update { it.copy(dialog = null) }
    }

    fun dismissConfirm() {
        _ui.update { it.copy(confirm = null) }
    }

    fun submitDialog(value: String) {
        val dlg = _ui.value.dialog ?: return
        val name = value.trim()
        if (name.isEmpty()) return
        val dir = _ui.value.state.path
        _ui.update { it.copy(dialog = null) }
        viewModelScope.launch {
            guard {
                when (dlg.action) {
                    DialogAction.CREATE_FILE -> repo.createFile(dir, name)
                    DialogAction.CREATE_FOLDER -> repo.createDir(dir, name)
                    DialogAction.RENAME -> dlg.description?.let { old -> repo.rename(join(dir, old), name) }
                }
                refresh()
            }
        }
    }

    fun requestDelete(paths: List<String>) {
        if (paths.isEmpty()) return
        val text = when (paths.size) {
            1 -> "Delete \"${baseName(paths[0])}\"? This cannot be undone."
            else -> "Delete ${paths.size} items? This cannot be undone."
        }
        _ui.update {
            it.copy(
                confirm = ConfirmState(
                    title = "Delete",
                    text = text,
                    confirmLabel = "Delete",
                    onConfirm = { performDelete(paths) },
                ),
            )
        }
    }

    private fun performDelete(paths: List<String>) {
        _ui.update { it.copy(confirm = null) }
        viewModelScope.launch {
            guard {
                val r = repo.delete(paths)
                clearSelection()
                refresh()
                snack(summarize("Deleted", r))
            }
        }
    }

    fun copySelection() {
        val paths = selectedPaths()
        if (paths.isEmpty()) return
        _ui.update { it.copy(pendingPaste = paths, pendingPasteWasCut = false, state = it.state.copy(selectMode = false, selection = emptySet())) }
        snack("Copied ${paths.size} item(s)")
    }

    fun cutSelection() {
        val paths = selectedPaths()
        if (paths.isEmpty()) return
        _ui.update { it.copy(pendingPaste = paths, pendingPasteWasCut = true, state = it.state.copy(selectMode = false, selection = emptySet())) }
        snack("Cut ${paths.size} item(s)")
    }

    fun hasClipboard(): Boolean = !_ui.value.pendingPaste.isNullOrEmpty()

    fun pasteHere() {
        val clip = _ui.value.pendingPaste ?: return
        val cut = _ui.value.pendingPasteWasCut
        val dest = _ui.value.state.path
        // Pasting into the same directory as the sources is a no-op for cut and
        // an expected "copy in place" for copy; the engine's rename collision
        // mode handles the naming.
        _ui.update { it.copy(pendingPaste = if (cut) null else clip, pendingPasteWasCut = false) }
        viewModelScope.launch {
            guard {
                val r = if (cut) repo.move(clip, dest, CollisionMode.RENAME) else repo.copy(clip, dest, CollisionMode.RENAME)
                refresh()
                snack(summarize(if (cut) "Moved" else "Copied", r))
            }
        }
    }

    fun copyInto(destDir: String) {
        val paths = selectedPaths().ifEmpty { return }
        viewModelScope.launch {
            guard {
                val r = repo.copy(paths, destDir, CollisionMode.RENAME)
                clearSelection()
                snack(summarize("Copied", r))
            }
        }
    }

    fun moveInto(destDir: String) {
        val paths = selectedPaths().ifEmpty { return }
        viewModelScope.launch {
            guard {
                val r = repo.move(paths, destDir, CollisionMode.RENAME)
                clearSelection()
                refresh()
                snack(summarize("Moved", r))
            }
        }
    }

    private fun summarize(verb: String, r: OpResult): String {
        val what = when {
            r.dirs > 0 && r.files > 0 -> "${r.files} files, ${r.dirs} folders"
            r.dirs > 0 -> "${r.dirs} folder(s)"
            else -> "${r.files} file(s)"
        }
        return when {
            r.cancelled -> "Cancelled"
            r.errorCount > 0 -> "$verb $what, ${r.errorCount} failed (${r.firstErrorCode})"
            else -> "$verb $what"
        }
    }

    // -------------------------------------------------------------- creating

    fun createFolderAndEnter(name: String) {
        submitDialog(name)
    }

    // ------------------------------------------------------------- open item

    fun isArchiveSync(path: String): Boolean = runCatching { FussNative.isArchive(path) }.getOrDefault(false)

    fun pathOf(name: String): String = join(_ui.value.state.path, name)

    // ------------------------------------------------------------- SAF roots

    fun attachSaf(uri: Uri) {
        val store = SafStore(getApplication())
        val root = store.add(uri) ?: run {
            snack("Could not use that folder")
            return
        }
        snack("Added ${root.label}")
    }

    companion object {
        fun sortLabel(key: Int, asc: Boolean): String = when (key) {
            SortSpec.SIZE -> if (asc) "Size ↑" else "Size ↓"
            SortSpec.TYPE -> if (asc) "Type ↑" else "Type ↓"
            SortSpec.MODIFIED -> if (asc) "Date ↑" else "Date ↓"
            else -> if (asc) "Name ↑" else "Name ↓"
        }
    }
}
