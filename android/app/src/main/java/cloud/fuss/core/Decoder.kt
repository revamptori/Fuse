package cloud.fuss.core

/**
 * Reader for the engine's binary wire format (see `rust/fuss-core/src/codec.rs`).
 *
 * Everything is little-endian. Strings are length-prefixed UTF-8 with a u32
 * length; entry records pack flags and the coarse file kind into single bytes.
 * Decoding is allocation-light: strings are the only heap objects created.
 */
internal class Decoder(private val buf: ByteArray) {
    private var pos = 0

    fun u8(): Int = buf[pos++].toInt() and 0xFF

    fun u32(): Int {
        val v = (buf[pos].toInt() and 0xFF) or
            ((buf[pos + 1].toInt() and 0xFF) shl 8) or
            ((buf[pos + 2].toInt() and 0xFF) shl 16) or
            ((buf[pos + 3].toInt() and 0xFF) shl 24)
        pos += 4
        return v
    }

    fun u64(): Long {
        var v = 0L
        for (i in 0 until 8) {
            v = v or ((buf[pos + i].toLong() and 0xFF) shl (8 * i))
        }
        pos += 8
        return v
    }

    fun i64(): Long = u64()

    fun u32Unsigned(): Long = u32().toLong() and 0xFFFF_FFFFL

    fun str(): String {
        val n = u32()
        val s = String(buf, pos, n, Charsets.UTF_8)
        pos += n
        return s
    }

    fun bytes(): ByteArray {
        val n = u32()
        val out = buf.copyOfRange(pos, pos + n)
        pos += n
        return out
    }

    fun entry(): FileEntry {
        val name = str()
        val flags = u8()
        val kind = u8()
        val size = u64()
        val modified = i64()
        val mode = u32()
        return FileEntry(
            name = name,
            isDir = flags and 1 != 0,
            isSymlink = flags and 2 != 0,
            statOk = flags and 4 != 0,
            kind = kind,
            size = size,
            modified = modified,
            mode = mode,
        )
    }

    val atEnd: Boolean get() = pos >= buf.size
}

/** Status byte precedes every encoded payload. */
internal object Wire {
    const val STATUS_OK = 0
    const val STATUS_ERROR = 1

    /** Decodes a result or throws the engine error. */
    inline fun <T> decode(bytes: ByteArray?, block: (Decoder) -> T): T {
        if (bytes == null) throw FussException("io", "Native call returned no data", "")
        val d = Decoder(bytes)
        when (d.u8()) {
            STATUS_OK -> {
                val payload = Decoder(d.bytes())
                return block(payload)
            }
            else -> {
                val code = d.str()
                val message = d.str()
                throw FussException(code, message, "")
            }
        }
    }
}

/**
 * One row in a directory listing. Deliberately a plain class with primitive
 * fields instead of data-class copies: a folder with 50 000 items allocates
 * exactly 50 000 of these and Compose can diff them by [key].
 */
class FileEntry(
    val name: String,
    val isDir: Boolean,
    val isSymlink: Boolean,
    val statOk: Boolean,
    val kind: Int,
    val size: Long,
    val modified: Long,
    val mode: Int,
) {
    val isHidden: Boolean get() = name.startsWith('.')
    val key: String get() = name

    companion object {
        const val KIND_DIRECTORY = 0
        const val KIND_IMAGE = 1
        const val KIND_VIDEO = 2
        const val KIND_AUDIO = 3
        const val KIND_TEXT = 4
        const val KIND_PDF = 5
        const val KIND_DOCUMENT = 6
        const val KIND_SPREADSHEET = 7
        const val KIND_PRESENTATION = 8
        const val KIND_ARCHIVE = 9
        const val KIND_APK = 10
        const val KIND_CODE = 11
        const val KIND_FONT = 12
        const val KIND_OTHER = 13
    }
}

/** A listing page plus progress metadata. */
class ListingPage(
    val entries: List<FileEntry>,
    val hasMore: Boolean,
    val totalScanned: Long,
)

/** Result of a copy/move/delete batch. */
class OpResult(
    val files: Long,
    val dirs: Long,
    val bytes: Long,
    val errorCount: Long,
    val skipped: Long,
    val firstErrorCode: String?,
    val firstErrorMessage: String?,
    val outputs: List<String>,
) {
    val cancelled: Boolean get() = firstErrorCode == "cancelled"
}

/** File metadata for the properties sheet. */
class FileProperties(
    val name: String,
    val path: String,
    val isDir: Boolean,
    val size: Long,
    val modified: Long,
    val mode: Int,
    val uid: Int,
    val gid: Int,
    val inode: Long,
    val hardLinks: Long,
    val isSymlink: Boolean,
    val symlinkTarget: String,
    val childCount: Long,
    val mime: String,
)

class StorageMount(
    val path: String,
    val fsType: String,
    val device: String,
    val isEmulated: Boolean,
    val label: String,
)

/** One hit in a recursive search. */
class SearchHit(val path: String, val entry: FileEntry)

class SearchResults(
    val hits: List<SearchHit>,
    val scanned: Long,
    val truncated: Boolean,
    val elapsedMs: Long,
)

class DirSizeResult(val files: Long, val dirs: Long, val bytes: Long)

class LargeFile(val path: String, val size: Long, val modified: Long)

class TypeStat(val kind: Int, val count: Long, val bytes: Long)

class DuplicateGroup(val size: Long, val wasted: Long, val paths: List<String>)

class DuplicateResults(val groups: List<DuplicateGroup>, val totalWasted: Long)

class ChildDirSize(val path: String, val bytes: Long, val files: Long, val dirs: Long)

class ArchiveEntry(
    val path: String,
    val isDir: Boolean,
    val size: Long,
    val modified: Long,
    val compressedSize: Long,
)

class ArchiveContents(
    val format: String,
    val totalBytes: Long,
    val totalCompressed: Long,
    val entries: List<ArchiveEntry>,
)

class ExtractResult(
    val files: Long,
    val dirs: Long,
    val bytes: Long,
    val skipped: Long,
    val errors: List<String>,
)
