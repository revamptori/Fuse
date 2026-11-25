package cloud.fuss.app

import android.content.Context
import java.io.File

/**
 * Favorites and recently visited folders, stored as one small text file in
 * private storage. Mirrors the Rust `prefs` module format on purpose: the same
 * file could be handed to the engine later without a migration.
 */
class RecentStore(context: Context) {

    private val file: File = File(context.filesDir, "recent.txt")
    private val favFile: File = File(context.filesDir, "favorites.txt")

    fun recent(): List<String> = read(file)
    fun favorites(): List<String> = read(favFile)

    fun add(path: String) {
        addTo(file, path, 30)
    }

    fun addFavorite(path: String) {
        addTo(favFile, path, 100)
    }

    fun removeFavorite(path: String) {
        val list = favorites().filter { it != path }
        write(favFile, list)
    }

    fun isFavorite(path: String): Boolean = favorites().contains(path)

    fun clearRecent() {
        write(file, emptyList())
    }

    private fun addTo(f: File, path: String, cap: Int) {
        val list = read(f).toMutableList()
        list.remove(path)
        list.add(0, path)
        while (list.size > cap) list.removeAt(list.size - 1)
        write(f, list)
    }

    private fun read(f: File): List<String> = runCatching {
        if (!f.exists()) emptyList() else f.readLines().filter { it.isNotBlank() }
    }.getOrDefault(emptyList())

    private fun write(f: File, lines: List<String>) {
        runCatching {
            val tmp = File(f.parentFile, f.name + ".tmp")
            tmp.writeText(lines.joinToString("\n"))
            tmp.renameTo(f)
        }
    }
}
