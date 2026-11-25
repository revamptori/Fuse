package cloud.fuss.ui

import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Android
import androidx.compose.material.icons.outlined.Archive
import androidx.compose.material.icons.outlined.AudioFile
import androidx.compose.material.icons.outlined.Code
import androidx.compose.material.icons.outlined.Description
import androidx.compose.material.icons.outlined.Folder
import androidx.compose.material.icons.outlined.Image
import androidx.compose.material.icons.outlined.InsertDriveFile
import androidx.compose.material.icons.outlined.Movie
import androidx.compose.material.icons.outlined.PictureAsPdf
import androidx.compose.material.icons.outlined.TableChart
import androidx.compose.material.icons.outlined.TextSnippet
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import cloud.fuss.core.FileEntry

/**
 * Icon + accent colour for a file kind. Uses the engine's classification (one
 * byte per entry) instead of an Android MIME lookup, so scrolling never does
 * string parsing per row.
 */
object FileIcons {

    fun icon(kind: Int): ImageVector = when (kind) {
        FileEntry.KIND_DIRECTORY -> Icons.Outlined.Folder
        FileEntry.KIND_IMAGE -> Icons.Outlined.Image
        FileEntry.KIND_VIDEO -> Icons.Outlined.Movie
        FileEntry.KIND_AUDIO -> Icons.Outlined.AudioFile
        FileEntry.KIND_TEXT -> Icons.Outlined.TextSnippet
        FileEntry.KIND_PDF -> Icons.Outlined.PictureAsPdf
        FileEntry.KIND_DOCUMENT -> Icons.Outlined.Description
        FileEntry.KIND_SPREADSHEET -> Icons.Outlined.TableChart
        FileEntry.KIND_PRESENTATION -> Icons.Outlined.Description
        FileEntry.KIND_ARCHIVE -> Icons.Outlined.Archive
        FileEntry.KIND_APK -> Icons.Outlined.Android
        FileEntry.KIND_CODE -> Icons.Outlined.Code
        FileEntry.KIND_FONT -> Icons.Outlined.TextSnippet
        else -> Icons.Outlined.InsertDriveFile
    }

    @Composable
    fun tint(kind: Int): Color = when (kind) {
        FileEntry.KIND_DIRECTORY -> MaterialTheme.colorScheme.primary
        FileEntry.KIND_IMAGE -> Color(0xFF4C7A5A)
        FileEntry.KIND_VIDEO -> Color(0xFF6A5A9E)
        FileEntry.KIND_AUDIO -> Color(0xFF9A6A2E)
        FileEntry.KIND_PDF -> Color(0xFFA64040)
        FileEntry.KIND_ARCHIVE -> Color(0xFF7A6A3A)
        FileEntry.KIND_APK -> Color(0xFF3E7A48)
        else -> MaterialTheme.colorScheme.onSurfaceVariant
    }
}
