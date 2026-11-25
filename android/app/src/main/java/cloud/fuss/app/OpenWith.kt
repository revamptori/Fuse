package cloud.fuss.app

import android.content.ClipData
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.webkit.MimeTypeMap
import androidx.core.content.FileProvider
import cloud.fuss.core.FileEntry
import java.io.File

/**
 * Opening and sharing files through the system.
 *
 * Fuss does not bundle viewers: it hands files to the system's default handler
 * via a content:// URI from our FileProvider, which is the only way Android 7+
 * allows sharing app-visible files. MIME detection prefers the engine's
 * extension table and falls back to Android's MimeTypeMap.
 */
object OpenWith {

    fun authority(context: Context): String = "${context.packageName}.fileprovider"

    fun uriFor(context: Context, path: String): Uri =
        FileProvider.getUriForFile(context, authority(context), File(path))

    fun mimeOf(context: Context, name: String, isDir: Boolean, engineMime: String): String {
        if (isDir) return "inode/directory"
        if (engineMime.isNotEmpty()) return engineMime
        val ext = name.substringAfterLast('.', "").lowercase()
        return MimeTypeMap.getSingleton().getMimeTypeFromExtension(ext) ?: "*/*"
    }

    /** Try to open a file with the system's default app. Returns false if none. */
    fun open(context: Context, path: String, entry: FileEntry): Boolean {
        val name = entry.name
        val mime = mimeOf(context, name, entry.isDir, "")
        val uri = runCatching { uriFor(context, path) }.getOrNull() ?: return false
        val intent = Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(uri, mime)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        }
        return runCatching { context.startActivity(intent) }.isSuccess
    }

    /** Share one or more paths through a chooser. */
    fun share(context: Context, paths: List<String>, mime: String = "*/*"): Boolean {
        if (paths.isEmpty()) return false
        val uris = paths.mapNotNull { runCatching { uriFor(context, it) }.getOrNull() }
        if (uris.isEmpty()) return false
        val intent = if (uris.size == 1) {
            Intent(Intent.ACTION_SEND).apply {
                type = mime
                putExtra(Intent.EXTRA_STREAM, uris[0])
            }
        } else {
            Intent(Intent.ACTION_SEND_MULTIPLE).apply {
                type = mime
                putParcelableArrayListExtra(Intent.EXTRA_STREAM, ArrayList(uris))
            }
        }
        intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        intent.clipData = ClipData.newRawUri(null, uris[0])
        val chooser = Intent.createChooser(intent, "Share").addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        return runCatching { context.startActivity(chooser) }.isSuccess
    }
}
