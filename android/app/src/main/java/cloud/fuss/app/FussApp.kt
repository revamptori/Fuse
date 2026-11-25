package cloud.fuss.app

import android.app.Application
import android.app.Activity
import android.os.Bundle
import android.widget.Toast

/**
 * Application entry point. Deliberately does nothing at startup except install
 * a crash reporter hook: no analytics, no DI framework, no eager initialisation.
 * A cold start should touch only the Application class and the first Activity.
 */
class FussApp : Application() {
    override fun onCreate() {
        super.onCreate()
        // Record the crash, then hand the exception to whatever handler was
        // installed before us (the platform one in production), so the system
        // still shows its normal crash UI and restarts the process properly.
        val previous = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { thread, throwable ->
            runCatching { CrashLog.record(this, throwable) }
            if (previous != null) {
                previous.uncaughtException(thread, throwable)
            } else {
                android.os.Process.killProcess(android.os.Process.myPid())
            }
        }
    }
}

/**
 * Minimal crash log: writes the stack trace to private storage so the user can
 * hand it over when reporting a problem. No upload, no network.
 */
object CrashLog {
    fun record(context: android.content.Context, t: Throwable) {
        runCatching {
            val f = java.io.File(context.filesDir, "last-crash.txt")
            f.writeText(android.util.Log.getStackTraceString(t))
        }
    }

    fun last(context: android.content.Context): String? =
        runCatching { java.io.File(context.filesDir, "last-crash.txt").takeIf { it.exists() }?.readText() }.getOrNull()
}
