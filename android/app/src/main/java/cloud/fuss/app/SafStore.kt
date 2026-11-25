package cloud.fuss.app

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.DocumentsContract
import androidx.documentfile.provider.DocumentFile

/**
 * Persisted Storage Access Framework directory grants.
 *
 * A granted tree is browsed by opening the document for each child and reading
 * through `/proc/self/fd/N`, which the Rust engine treats like any other path.
 * That keeps one filesystem engine for both plain and SAF storage instead of a
 * parallel implementation.
 *
 * Only *directories* are grantable here; single files the user picks are routed
 * to the system previewer, because the engine's operations need a stable path
 * hierarchy to be meaningful.
 */
class SafStore(private val context: Context) {

    private val prefs = context.getSharedPreferences("saf_roots", Context.MODE_PRIVATE)

    data class SafRoot(val label: String, val uri: String)

    fun roots(): List<SafRoot> {
        val raw = prefs.getStringSet("roots", emptySet()) ?: emptySet()
        return raw.mapNotNull { entry ->
            val idx = entry.indexOf('|')
            if (idx <= 0) null else SafRoot(entry.substring(0, idx), entry.substring(idx + 1))
        }.sortedBy { it.label }
    }

    /** Records a tree URI the user granted with [Intent.ACTION_OPEN_DOCUMENT_TREE]. */
    fun add(uri: Uri): SafRoot? {
        if (!DocumentsContract.isTreeUri(uri)) return null
        val takeFlags = Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
        runCatching { context.contentResolver.takePersistableUriPermission(uri, takeFlags) }
        val doc = DocumentFile.fromTreeUri(context, uri) ?: return null
        val label = doc.name ?: uri.lastPathSegment ?: "SAF folder"
        val set = (prefs.getStringSet("roots", emptySet()) ?: emptySet()).toMutableSet()
        set.add("$label|${uri}")
        prefs.edit().putStringSet("roots", set).apply()
        return SafRoot(label, uri.toString())
    }

    fun remove(root: SafRoot) {
        val set = (prefs.getStringSet("roots", emptySet()) ?: emptySet()).toMutableSet()
        set.removeIf { it.endsWith("|${root.uri}") }
        prefs.edit().putStringSet("roots", set).apply()
        runCatching {
            context.contentResolver.releasePersistableUriPermission(
                Uri.parse(root.uri),
                Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION,
            )
        }
    }

    fun document(uri: String): DocumentFile? = DocumentFile.fromTreeUri(context, Uri.parse(uri))
}
