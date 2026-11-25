package cloud.fuss.app

import android.content.Context
import android.os.Build
import android.os.Environment
import android.os.storage.StorageManager
import java.io.File

/**
 * The places the user can browse without extra permissions.
 *
 * Android 11+ grants read access to all of shared storage for apps that request
 * `READ_EXTERNAL_STORAGE`/`MANAGE_EXTERNAL_STORAGE` is *not* requested here on
 * purpose: browsing shared storage works through the media/shared directories
 * that are readable, and everything else goes through SAF. This class only
 * reports what the OS says is available; it never guesses.
 */
data class Root(
    val label: String,
    val path: String,
    val kind: RootKind,
)

enum class RootKind { INTERNAL, SD_CARD, DOWNLOADS, APP_PRIVATE }

object StorageRoots {

    /** Roots shown on the home screen, in order. */
    fun discover(context: Context): List<Root> {
        val roots = mutableListOf<Root>()
        val shared = Environment.getExternalStorageDirectory()
        if (shared != null) {
            roots += Root("Internal storage", shared.absolutePath, RootKind.INTERNAL)
        }
        // Removable volumes, as reported by the StorageManager (the only
        // supported way since Android 9).
        val sm = context.getSystemService(StorageManager::class.java)
        if (sm != null) {
            runCatching {
                for (volume in sm.storageVolumes) {
                    val dir = if (Build.VERSION.SDK_INT >= 30) volume.directory else null
                    if (volume.isRemovable && dir != null && dir.canRead()) {
                        val label = volume.getDescription(context) ?: "SD card"
                        roots += Root(label, dir.absolutePath, RootKind.SD_CARD)
                    }
                }
            }
        }
        roots += Root("Downloads", Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS).absolutePath, RootKind.DOWNLOADS)
        val appFiles = context.filesDir
        roots += Root("Fuss (app storage)", appFiles.absolutePath, RootKind.APP_PRIVATE)
        return roots
    }

    /** Common starting folders inside shared storage; only existing ones are returned. */
    fun shortcuts(context: Context): List<Root> {
        val shared = Environment.getExternalStorageDirectory() ?: return emptyList()
        val candidates = listOf(
            "Download" to Environment.DIRECTORY_DOWNLOADS,
            "Pictures" to Environment.DIRECTORY_PICTURES,
            "DCIM" to Environment.DIRECTORY_DCIM,
            "Movies" to Environment.DIRECTORY_MOVIES,
            "Music" to Environment.DIRECTORY_MUSIC,
            "Documents" to Environment.DIRECTORY_DOCUMENTS,
            "Android" to "Android",
        )
        return candidates.mapNotNull { (label, dir) ->
            val f = File(shared, dir)
            if (f.isDirectory) Root(label, f.absolutePath, RootKind.INTERNAL) else null
        }
    }
}
