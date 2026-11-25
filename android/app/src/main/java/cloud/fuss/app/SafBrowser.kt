package cloud.fuss.app

import android.content.Context
import android.net.Uri
import androidx.documentfile.provider.DocumentFile
import cloud.fuss.core.FileEntry
import java.io.File

/**
 * Minimal SAF browsing layer.
 *
 * The Rust engine works on paths, and SAF documents have no filesystem path, so
 * SAF trees are walked through [DocumentFile] here in Kotlin. That is the
 * "Android-specific integration" half of the architecture: the engine owns
 * everything that has a path, the platform layer owns content:// handles.
 *
 * Copying out of SAF streams through a 64 KB buffer into a real directory, so
 * the file immediately becomes visible to the engine and to other apps. Nothing
 * is buffered in memory.
 */
object SafBrowser {

    data class SafNode(
        val uri: String,
        val name: String,
        val isDir: Boolean,
        val size: Long,
    )

    /** Lists the direct children of a document tree URI. */
    fun children(context: Context, treeUri: String): List<SafNode> {
        val dir = DocumentFile.fromTreeUri(context, Uri.parse(treeUri)) ?: return emptyList()
        if (!dir.isDirectory || !dir.canRead()) return emptyList()
        val out = ArrayList<SafNode>()
        for (child in dir.listFiles()) {
            out.add(SafNode(child.uri.toString(), child.name ?: "(unnamed)", child.isDirectory, child.length()))
        }
        return out.sortedWith(compareBy({ !it.isDir }, { it.name.lowercase() }))
    }

    fun document(context: Context, uri: String): DocumentFile? =
        DocumentFile.fromSingleUri(context, Uri.parse(uri))

    /**
     * Copies one SAF document into [destDir], keeping the display name and
     * avoiding collisions with a numeric suffix. Returns the created file.
     */
    fun copyTo(context: Context, uri: String, destDir: File): File? {
        val doc = DocumentFile.fromSingleUri(context, Uri.parse(uri)) ?: return null
        if (!doc.isFile) return null
        val name = doc.name ?: "document"
        destDir.mkdirs()
        var target = File(destDir, name)
        var n = 1
        while (target.exists()) {
            val base = name.substringBeforeLast('.', name)
            val ext = name.substringAfterLast('.', "")
            target = File(destDir, if (ext.isEmpty()) "$base ($n)" else "$base ($n).$ext")
            n++
        }
        val input = context.contentResolver.openInputStream(doc.uri) ?: return null
        input.use { ins ->
            target.outputStream().use { outs ->
                val buf = ByteArray(64 * 1024)
                var total = 0L
                while (true) {
                    val r = ins.read(buf)
                    if (r <= 0) break
                    outs.write(buf, 0, r)
                    total += r
                }
                outs.flush()
                if (total == 0L && doc.length() > 0) {
                    // Reported size but produced nothing: treat as failure so the
                    // user is never told a broken copy succeeded.
                    outs.close()
                    target.delete()
                    return null
                }
            }
        }
        return target
    }

    /** Best-effort delete of a SAF child; returns the engine-style result. */
    fun delete(context: Context, uri: String): Boolean {
        val doc = DocumentFile.fromSingleUri(context, Uri.parse(uri)) ?: return false
        return runCatching { doc.delete() }.getOrDefault(false)
    }

    /**
     * Converts a SAF node to a [FileEntry] so the UI can reuse the same row
     * rendering (icons, kinds). Sizes come from the provider, which may report
     * 0 or -1 for unknown.
     */
    fun asEntry(node: SafNode): FileEntry = FileEntry(
        name = node.name,
        isDir = node.isDir,
        isSymlink = false,
        statOk = node.size >= 0,
        kind = if (node.isDir) FileEntry.KIND_DIRECTORY else kindOf(node.name),
        size = if (node.size > 0) node.size else 0L,
        modified = 0L,
        mode = 0,
    )

    private fun kindOf(name: String): Int {
        val ext = name.substringAfterLast('.', "").lowercase()
        return when (ext) {
            "jpg", "jpeg", "png", "gif", "webp", "bmp", "heic", "heif", "avif" -> FileEntry.KIND_IMAGE
            "mp4", "mkv", "webm", "mov", "avi", "3gp", "m4v" -> FileEntry.KIND_VIDEO
            "mp3", "flac", "ogg", "opus", "m4a", "wav", "aac", "mid", "mka" -> FileEntry.KIND_AUDIO
            "pdf" -> FileEntry.KIND_PDF
            "doc", "docx", "odt", "rtf", "epub" -> FileEntry.KIND_DOCUMENT
            "xls", "xlsx", "ods", "csv" -> FileEntry.KIND_SPREADSHEET
            "ppt", "pptx", "odp" -> FileEntry.KIND_PRESENTATION
            "zip", "tar", "gz", "tgz", "7z", "rar", "bz2", "xz", "zst" -> FileEntry.KIND_ARCHIVE
            "apk", "apks", "xapk" -> FileEntry.KIND_APK
            "txt", "md", "log", "json", "xml", "html", "htm" -> FileEntry.KIND_TEXT
            "kt", "kts", "rs", "py", "js", "ts", "c", "h", "cpp", "java", "sh", "toml", "yml", "yaml" -> FileEntry.KIND_CODE
            "ttf", "otf", "woff", "woff2" -> FileEntry.KIND_FONT
            else -> FileEntry.KIND_OTHER
        }
    }
}
