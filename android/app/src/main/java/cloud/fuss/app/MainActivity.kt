package cloud.fuss.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import cloud.fuss.ui.FussNav

/**
 * Single Activity. It sets up the edge-to-edge window and installs the Compose
 * tree; all navigation lives in [cloud.fuss.ui.FussNav].
 */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val prefs = getSharedPreferences("fuss", 0)
        val dark = if (prefs.contains("dark")) prefs.getBoolean("dark", false) else null
        val dynamic = prefs.getBoolean("dynamic", false)
        setContent {
            FussNav(darkOverride = dark, dynamic = dynamic)
        }
    }
}
