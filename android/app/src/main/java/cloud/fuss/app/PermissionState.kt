package cloud.fuss.app

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.os.Environment
import androidx.core.content.ContextCompat

/**
 * Permission *status* only. Fuss deliberately does not request broad storage
 * permissions: browsing shared storage relies on the media permissions that
 * Android grants per-directory on 11+, and anything else uses SAF.
 *
 * On Android 11 and 12 `READ_EXTERNAL_STORAGE` still gates some public folders;
 * we ask for it with the narrowest possible scope and degrade to SAF when it is
 * denied instead of failing.
 */
object PermissionState {

    val storagePermission: String
        get() = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            Manifest.permission.READ_MEDIA_IMAGES
        } else {
            Manifest.permission.READ_EXTERNAL_STORAGE
        }

    fun hasMediaRead(context: Context): Boolean =
        ContextCompat.checkSelfPermission(context, storagePermission) == PackageManager.PERMISSION_GRANTED

    /**
     * Whether the app can enumerate the given directory directly, without SAF.
     * This is a *probe*, not a permission claim: it simply tries to list it.
     */
    fun canRead(context: Context, path: String): Boolean {
        val f = java.io.File(path)
        if (!f.isDirectory) return false
        return runCatching { f.list() != null }.getOrDefault(false)
    }

    /** True when shared storage exists and is mounted. */
    fun sharedStorageMounted(): Boolean = Environment.getExternalStorageState() == Environment.MEDIA_MOUNTED

    /** True when the device runs a version where SAF is the only general answer. */
    fun requiresSaf(): Boolean = Build.VERSION.SDK_INT >= Build.VERSION_CODES.R
}
