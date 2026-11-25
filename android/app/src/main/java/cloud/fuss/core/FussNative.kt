package cloud.fuss.core

/**
 * The complete JNI surface of the Rust engine.
 *
 * Every function here maps 1:1 to an `extern "system"` function in
 * `rust/fuss-jni/src/lib.rs`. Calls that can take longer than a frame are
 * `*Blocking` by nature (copy/delete/extract/search) and must be invoked on
 * [Dispatchers.IO] by the repository layer -- never from the main thread.
 *
 * Results are encoded byte buffers decoded by [Decoder]; see the Rust `codec`
 * module for the wire format.
 */
object FussNative {

    init {
        System.loadLibrary("fuss_core")
    }

    /** Engine + rustc version string. */
    external fun version(): String

    /** Runs the native self-test; returns "name=ok;name=fail" style report. */
    external fun selfTest(): String

    // --- cancellation -------------------------------------------------------

    external fun newCancelToken(): Long
    external fun cancelToken(handle: Long)
    external fun freeCancelToken(handle: Long)
    external fun progress(handle: Long): Long
    external fun progressTotal(handle: Long): Long

    // --- browsing -----------------------------------------------------------

    /** Wire: [hasMore:u8][count:u32][entries...][totalScanned:u64] */
    external fun listDir(
        path: String,
        showHidden: Boolean,
        sortKey: Int,
        ascending: Boolean,
        limit: Int,
        offset: Int,
    ): ByteArray?

    /** Wire: one entry record. */
    external fun statEntry(path: String): ByteArray?

    external fun exists(path: String): Boolean
    external fun isDirectory(path: String): Boolean
    external fun isReadable(path: String): Boolean
    external fun canonicalize(path: String): String
    external fun mimeType(name: String, isDir: Boolean): String
    external fun freeSpace(path: String): Long
    external fun totalSpace(path: String): Long

    /** Wire: [count:u32][path, fsType, device, isEmulated...] */
    external fun mounts(): ByteArray?

    // --- mutation -----------------------------------------------------------

    external fun createFile(dir: String, name: String): String
    external fun createDir(dir: String, name: String): String
    external fun rename(path: String, newName: String): String

    /** Wire: [files:u64][dirs:u64][bytes:u64][errors:u64][skipped:u64][errFlag:u8][code][msg][count:u32][paths...] */
    external fun copyBatch(sources: Array<String>, destDir: String, mode: Int, cancelHandle: Long): ByteArray?

    external fun moveBatch(sources: Array<String>, destDir: String, mode: Int, cancelHandle: Long): ByteArray?
    external fun deleteBatch(sources: Array<String>, cancelHandle: Long): ByteArray?

    // --- properties ---------------------------------------------------------

    external fun properties(path: String): ByteArray?
    external fun dirSize(path: String, cancelHandle: Long): ByteArray?

    // --- search -------------------------------------------------------------

    external fun search(root: String, query: String, includeHidden: Boolean, maxResults: Int, cancelHandle: Long): ByteArray?

    // --- analysis -----------------------------------------------------------

    external fun largestFiles(root: String, limit: Int, cancelHandle: Long): ByteArray?
    external fun emptyDirs(root: String, limit: Int, cancelHandle: Long): ByteArray?
    external fun typeStats(root: String, cancelHandle: Long): ByteArray?
    external fun duplicates(root: String, cancelHandle: Long): ByteArray?
    external fun childDirSizes(root: String, cancelHandle: Long): ByteArray?

    // --- archives -----------------------------------------------------------

    external fun listArchive(path: String, cancelHandle: Long): ByteArray?
    external fun extractArchive(path: String, dest: String, cancelHandle: Long): ByteArray?
    external fun createArchive(sources: Array<String>, archivePath: String, format: String, cancelHandle: Long): ByteArray?
    external fun isArchive(path: String): Boolean
}

/** Thrown when the engine reports a failure; [code] is stable and machine readable. */
class FussException(
    val code: String,
    val engineMessage: String,
    val path: String,
) : java.io.IOException(if (path.isEmpty()) engineMessage else "$engineMessage ($path)") {

    val isCancellation: Boolean get() = code == "cancelled"

    /** Short, human text for a snackbar/dialog. */
    fun userMessage(): String = when (code) {
        "not_found" -> "Item no longer exists"
        "permission_denied" -> "Permission denied"
        "already_exists" -> "An item with that name already exists"
        "not_a_directory" -> "Not a folder"
        "not_empty" -> "Folder is not empty"
        "no_space" -> "Not enough storage space"
        "read_only" -> "This storage is read-only"
        "invalid_name" -> "Invalid name"
        "invalid_path" -> "Invalid path"
        "invalid_argument" -> "Invalid request"
        "cross_device" -> "Moving across storage is not possible here"
        "cancelled" -> "Cancelled"
        "unsupported" -> "Not supported"
        "corrupt_archive" -> "Archive is damaged"
        "busy" -> "File is in use"
        else -> if (engineMessage.isEmpty()) "Operation failed" else engineMessage
    }
}
