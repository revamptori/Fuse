package cloud.fuss.core

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Thin, coroutine-friendly facade over [FussNative].
 *
 * Rules encoded here:
 * * every native call runs on [Dispatchers.IO];
 * * every operation that can run long owns a cancel handle that is released in
 *   a `finally` block, so the native handle table cannot leak;
 * * results are decoded eagerly on the IO thread and handed to the UI as plain
 *   immutable objects -- the UI never sees a ByteArray.
 */
class FsRepository {

    // ---- browsing ----------------------------------------------------------

    suspend fun list(
        path: String,
        showHidden: Boolean,
        sort: SortSpec,
        limit: Int = 0,
        offset: Int = 0,
    ): ListingPage = withContext(Dispatchers.IO) {
        Wire.decode(
            FussNative.listDir(path, showHidden, sort.key, sort.ascending, limit, offset)
        ) { d ->
            val hasMore = d.u8() != 0
            val count = d.u32()
            val entries = ArrayList<FileEntry>(count)
            repeat(count) { entries.add(d.entry()) }
            // Total scanned keeps its meaning even when a page is returned.
            val totalScanned = if (!d.atEnd) d.u64() else entries.size.toLong()
            ListingPage(entries, hasMore, totalScanned)
        }
    }

    suspend fun stat(path: String): FileEntry = withContext(Dispatchers.IO) {
        Wire.decode(FussNative.statEntry(path)) { it.entry() }
    }

    suspend fun exists(path: String): Boolean = withContext(Dispatchers.IO) { FussNative.exists(path) }
    suspend fun isDirectory(path: String): Boolean = withContext(Dispatchers.IO) { FussNative.isDirectory(path) }
    suspend fun isReadable(path: String): Boolean = withContext(Dispatchers.IO) { FussNative.isReadable(path) }
    suspend fun canonical(path: String): String = withContext(Dispatchers.IO) { FussNative.canonicalize(path) }
    suspend fun mimeOf(name: String, isDir: Boolean): String = withContext(Dispatchers.IO) {
        FussNative.mimeType(name, isDir)
    }

    suspend fun freeSpace(path: String): Long = withContext(Dispatchers.IO) { FussNative.freeSpace(path) }
    suspend fun totalSpace(path: String): Long = withContext(Dispatchers.IO) { FussNative.totalSpace(path) }

    suspend fun mounts(): List<StorageMount> = withContext(Dispatchers.IO) {
        val bytes = FussNative.mounts() ?: return@withContext emptyList()
        val d = Decoder(bytes)
        val count = d.u32()
        (0 until count).map {
            val path = d.str()
            val fs = d.str()
            val dev = d.str()
            val emulated = d.u8() != 0
            StorageMount(path, fs, dev, emulated, labelFor(path, emulated))
        }
    }

    private fun labelFor(path: String, emulated: Boolean): String = when {
        emulated && path == "/storage/emulated/0" -> "Internal storage"
        emulated -> "Emulated storage"
        path.startsWith("/storage/") -> "SD card"
        else -> path.substringAfterLast('/')
    }

    // ---- mutation ----------------------------------------------------------

    suspend fun createFile(dir: String, name: String): String = withContext(Dispatchers.IO) {
        FussNative.createFile(dir, name)
    }

    suspend fun createDir(dir: String, name: String): String = withContext(Dispatchers.IO) {
        FussNative.createDir(dir, name)
    }

    suspend fun rename(path: String, newName: String): String = withContext(Dispatchers.IO) {
        FussNative.rename(path, newName)
    }

    suspend fun copy(sources: List<String>, destDir: String, mode: CollisionMode): OpResult =
        withContext(Dispatchers.IO) {
            withToken { handle -> copyWithHandle(sources, destDir, mode, handle) }
        }

    /**
     * Copy using a cancel handle owned by the caller.
     *
     * Exists for tests that need to cancel mid-transfer from another thread; the
     * handle is *not* freed here, because the caller may still be waiting on it.
     */
    suspend fun copyWithHandle(
        sources: List<String>,
        destDir: String,
        mode: CollisionMode,
        handle: Long,
    ): OpResult = withContext(Dispatchers.IO) {
        Wire.decode(FussNative.copyBatch(sources.toTypedArray(), destDir, mode.wire, handle)) {
            decodeOpResult(it)
        }
    }

    suspend fun move(sources: List<String>, destDir: String, mode: CollisionMode): OpResult =
        withContext(Dispatchers.IO) {
            withToken { handle ->
                Wire.decode(FussNative.moveBatch(sources.toTypedArray(), destDir, mode.wire, handle)) {
                    decodeOpResult(it)
                }
            }
        }

    suspend fun delete(sources: List<String>): OpResult = withContext(Dispatchers.IO) {
        withToken { handle ->
            Wire.decode(FussNative.deleteBatch(sources.toTypedArray(), handle)) { decodeOpResult(it) }
        }
    }

    private fun decodeOpResult(d: Decoder): OpResult {
        val files = d.u64()
        val dirs = d.u64()
        val bytes = d.u64()
        val errors = d.u64()
        val skipped = d.u64()
        val hasError = d.u8() != 0
        val code = d.str()
        val message = d.str()
        val n = d.u32()
        val outputs = ArrayList<String>(n)
        repeat(n) { outputs.add(d.str()) }
        return OpResult(files, dirs, bytes, errors, skipped, if (hasError) code else null, if (hasError) message else null, outputs)
    }

    // ---- properties --------------------------------------------------------

    suspend fun properties(path: String): FileProperties = withContext(Dispatchers.IO) {
        Wire.decode(FussNative.properties(path)) { d ->
            FileProperties(
                name = d.str(), path = d.str(), isDir = d.u8() != 0,
                size = d.u64(), modified = d.i64(), mode = d.u32(),
                uid = d.u32(), gid = d.u32(), inode = d.u64(), hardLinks = d.u64(),
                isSymlink = d.u8() != 0, symlinkTarget = d.str(),
                childCount = d.u64(), mime = d.str(),
            )
        }
    }

    suspend fun dirSize(path: String): DirSizeResult = withContext(Dispatchers.IO) {
        withToken { handle ->
            Wire.decode(FussNative.dirSize(path, handle)) { d ->
                DirSizeResult(d.u64(), d.u64(), d.u64())
            }
        }
    }

    // ---- search ------------------------------------------------------------

    suspend fun search(root: String, query: String, includeHidden: Boolean, maxResults: Int = 20_000): SearchResults =
        withContext(Dispatchers.IO) {
            withToken { handle ->
                Wire.decode(FussNative.search(root, query, includeHidden, maxResults, handle)) { d ->
                    val n = d.u32()
                    val hits = ArrayList<SearchHit>(n)
                    repeat(n) {
                        val path = d.str()
                        hits.add(SearchHit(path, d.entry()))
                    }
                    val scanned = d.u64()
                    val truncated = d.u8() != 0
                    val elapsed = d.u64()
                    SearchResults(hits, scanned, truncated, elapsed)
                }
            }
        }

    // ---- analysis ----------------------------------------------------------

    suspend fun largestFiles(root: String, limit: Int = 100): List<LargeFile> = withContext(Dispatchers.IO) {
        withToken { handle ->
            Wire.decode(FussNative.largestFiles(root, limit, handle)) { d ->
                val n = d.u32()
                (0 until n).map { LargeFile(d.str(), d.u64(), d.i64()) }
            }
        }
    }

    suspend fun emptyDirs(root: String, limit: Int = 500): List<String> = withContext(Dispatchers.IO) {
        withToken { handle ->
            Wire.decode(FussNative.emptyDirs(root, limit, handle)) { d ->
                val n = d.u32()
                (0 until n).map { d.str() }
            }
        }
    }

    suspend fun typeStats(root: String): List<TypeStat> = withContext(Dispatchers.IO) {
        withToken { handle ->
            Wire.decode(FussNative.typeStats(root, handle)) { d ->
                val n = d.u32()
                (0 until n).map { TypeStat(d.u8(), d.u64(), d.u64()) }
            }
        }
    }

    suspend fun duplicates(root: String): DuplicateResults = withContext(Dispatchers.IO) {
        withToken { handle ->
            Wire.decode(FussNative.duplicates(root, handle)) { d ->
                val n = d.u32()
                val groups = ArrayList<DuplicateGroup>(n)
                repeat(n) {
                    val size = d.u64()
                    val wasted = d.u64()
                    val pc = d.u32()
                    val paths = ArrayList<String>(pc)
                    repeat(pc) { paths.add(d.str()) }
                    groups.add(DuplicateGroup(size, wasted, paths))
                }
                val total = d.u64()
                DuplicateResults(groups, total)
            }
        }
    }

    suspend fun childDirSizes(root: String): List<ChildDirSize> = withContext(Dispatchers.IO) {
        withToken { handle ->
            Wire.decode(FussNative.childDirSizes(root, handle)) { d ->
                val n = d.u32()
                (0 until n).map { ChildDirSize(d.str(), d.u64(), d.u64(), d.u64()) }
            }
        }
    }

    // ---- archives ----------------------------------------------------------

    suspend fun listArchive(path: String): ArchiveContents = withContext(Dispatchers.IO) {
        withToken { handle ->
            Wire.decode(FussNative.listArchive(path, handle)) { d ->
                val format = d.str()
                val totalBytes = d.u64()
                val totalCompressed = d.u64()
                val n = d.u32()
                val entries = ArrayList<ArchiveEntry>(n)
                repeat(n) {
                    val p = d.str()
                    val isDir = d.u8() != 0
                    val size = d.u64()
                    val modified = d.i64()
                    val compressed = d.u64()
                    entries.add(ArchiveEntry(p, isDir, size, modified, compressed))
                }
                ArchiveContents(format, totalBytes, totalCompressed, entries)
            }
        }
    }

    suspend fun extractArchive(path: String, dest: String): ExtractResult = withContext(Dispatchers.IO) {
        withToken { handle ->
            Wire.decode(FussNative.extractArchive(path, dest, handle)) { d ->
                val files = d.u64()
                val dirs = d.u64()
                val bytes = d.u64()
                val skipped = d.u64()
                val n = d.u32()
                val errors = ArrayList<String>(n)
                repeat(n) { errors.add(d.str()) }
                ExtractResult(files, dirs, bytes, skipped, errors)
            }
        }
    }

    suspend fun createArchive(sources: List<String>, archivePath: String, format: String): Long =
        withContext(Dispatchers.IO) {
            withToken { handle ->
                Wire.decode(FussNative.createArchive(sources.toTypedArray(), archivePath, format, handle)) {
                    it.u64()
                }
            }
        }

    suspend fun isArchive(path: String): Boolean = withContext(Dispatchers.IO) { FussNative.isArchive(path) }

    // ---- helpers -----------------------------------------------------------

    /** Runs [block] with a native cancel handle, freeing it afterwards. */
    private inline fun <T> withToken(block: (Long) -> T): T {
        val handle = FussNative.newCancelToken()
        try {
            return block(handle)
        } finally {
            FussNative.freeCancelToken(handle)
        }
    }
}

/** Sort specification matching the engine's `SortKey`. */
data class SortSpec(val key: Int, val ascending: Boolean) {
    companion object {
        const val NAME = 0
        const val SIZE = 1
        const val TYPE = 2
        const val MODIFIED = 3
        val DEFAULT = SortSpec(NAME, true)
    }
}

/** What to do when a destination already exists (matches Rust `CollisionMode`). */
enum class CollisionMode(val wire: Int) {
    FAIL(0),
    OVERWRITE(1),
    RENAME(2),
}
