package cloud.fuss.ui

import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/** Human formatting helpers used across the UI. */
object Format {

    private val sizeUnits = arrayOf("B", "KB", "MB", "GB", "TB")

    /** 1.42 MB style sizes, two significant decimals max. */
    fun size(bytes: Long): String {
        if (bytes < 0) return "-"
        if (bytes < 1024) return "$bytes B"
        var v = bytes.toDouble()
        var unit = 0
        while (v >= 1024 && unit < sizeUnits.lastIndex) {
            v /= 1024.0
            unit++
        }
        return if (v >= 100) {
            String.format(Locale.US, "%.0f %s", v, sizeUnits[unit])
        } else if (v >= 10) {
            String.format(Locale.US, "%.1f %s", v, sizeUnits[unit])
        } else {
            String.format(Locale.US, "%.2f %s", v, sizeUnits[unit])
        }
    }

    private val dateFormat = SimpleDateFormat("d MMM yyyy, HH:mm", Locale.getDefault())
    private val shortDateFormat = SimpleDateFormat("d MMM yyyy", Locale.getDefault())

    fun dateTime(epochSeconds: Long): String =
        if (epochSeconds <= 0) "Unknown" else dateFormat.format(Date(epochSeconds * 1000))

    fun date(epochSeconds: Long): String =
        if (epochSeconds <= 0) "Unknown" else shortDateFormat.format(Date(epochSeconds * 1000))

    /** "Today 14:03", "Yesterday 09:11", "3 Mar 2024". */
    fun relativeDate(epochSeconds: Long): String {
        if (epochSeconds <= 0) return "Unknown"
        val now = System.currentTimeMillis() / 1000
        val diff = now - epochSeconds
        val timeOfDay = SimpleDateFormat("HH:mm", Locale.getDefault()).format(Date(epochSeconds * 1000))
        return when {
            diff < 0 -> date(epochSeconds)
            diff < 86_400 -> "Today $timeOfDay"
            diff < 172_800 -> "Yesterday $timeOfDay"
            diff < 604_800 -> SimpleDateFormat("EEE, HH:mm", Locale.getDefault()).format(Date(epochSeconds * 1000))
            else -> date(epochSeconds)
        }
    }

    /** rwxr-xr-x from a POSIX mode. */
    fun permissions(mode: Int): String {
        val sb = StringBuilder(10)
        val type = when (mode and 0xF000) {
            0x4000 -> 'd'
            0xA000 -> 'l'
            0x1000 -> 'p'
            0x2000 -> 'c'
            0x6000 -> 'b'
            0xC000 -> 's'
            else -> '-'
        }
        sb.append(type)
        val perms = arrayOf(
            0x100 to 'r', 0x80 to 'w', 0x40 to 'x',
            0x20 to 'r', 0x10 to 'w', 0x8 to 'x',
            0x4 to 'r', 0x2 to 'w', 0x1 to 'x',
        )
        for ((bit, ch) in perms) sb.append(if (mode and bit != 0) ch else '-')
        return sb.toString()
    }

    /** MIME to a short type label for the type column. */
    fun typeLabel(mime: String, isDir: Boolean): String {
        if (isDir) return "Folder"
        if (mime.isEmpty()) return "File"
        return mime.substringAfter('/').replace("x-", "").replace("vnd.", "").replace(".", " ").uppercase(Locale.US).take(12)
    }
}
