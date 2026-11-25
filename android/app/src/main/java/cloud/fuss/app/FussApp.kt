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
        Thread.setDefaultUncaughtExceptionHandler { thread, throwable ->
            CrashLog.record(this, throwable)
            android.os.Process.killProcess(android.os.Process.myPid())
            kotlin.system.exitProcess(10)
        }
        uncaughtThread = thread
    }

    companion object {
        private var uncaughtThread: Thread? = null
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
