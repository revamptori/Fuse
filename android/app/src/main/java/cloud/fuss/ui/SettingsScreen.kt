package cloud.fuss.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import cloud.fuss.app.CrashLog
import cloud.fuss.app.RecentStore
import cloud.fuss.core.FussNative

/**
 * Settings and diagnostics. The self-test button runs every engine path on a
 * temporary tree and shows the raw report: no hidden "it works" claims.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(onBack: () -> Unit) {
    val context = LocalContext.current
    val prefs = context.getSharedPreferences("fuss", 0)
    var followSystem by remember { mutableStateOf(!prefs.contains("dark")) }
    var dark by remember { mutableStateOf(prefs.getBoolean("dark", false)) }
    var dynamic by remember { mutableStateOf(prefs.getBoolean("dynamic", false)) }
    var report by remember { mutableStateOf("") }
    var crashLog by remember { mutableStateOf("") }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Settings") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Outlined.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
    ) { padding ->
        Column(
            Modifier
                .padding(padding)
                .fillMaxSize()
                .verticalScroll(rememberScrollState()),
        ) {
            SettingSwitch(
                label = "Follow system theme",
                checked = followSystem,
                onChange = {
                    followSystem = it
                    val editor = prefs.edit()
                    if (it) editor.remove("dark") else editor.putBoolean("dark", dark)
                    editor.apply()
                },
            )
            if (!followSystem) {
                SettingSwitch(
                    label = "Dark theme",
                    checked = dark,
                    onChange = {
                        dark = it
                        prefs.edit().putBoolean("dark", it).apply()
                    },
                )
            }
            SettingSwitch(
                label = "Dynamic colour (Android 12+)",
                checked = dynamic,
                onChange = {
                    dynamic = it
                    prefs.edit().putBoolean("dynamic", it).apply()
                },
            )
            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            InfoBlock(
                title = "Privacy",
                body = "Fuss has no network permission, no analytics, no ads and no background " +
                    "services. It requests media-read access only so thumbnails and previews can " +
                    "be shown, and reaches everything else through folders you grant with the " +
                    "system picker.",
            )
            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            Column(Modifier.padding(16.dp)) {
                Text("Diagnostics", style = MaterialTheme.typography.titleSmall)
                Spacer(Modifier.height(6.dp))
                Text(engineLine(), style = MaterialTheme.typography.bodySmall)
                Spacer(Modifier.height(4.dp))
                TextButton(onClick = { report = engineSelfTest() }) {
                    Text("Run engine self-test")
                }
                if (report.isNotEmpty()) {
                    Text(report, style = MaterialTheme.typography.bodySmall)
                }
            }
            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            Column(Modifier.padding(16.dp)) {
                Text("History", style = MaterialTheme.typography.titleSmall)
                TextButton(onClick = { RecentStore(context).clearRecent() }) {
                    Text("Clear recent folders")
                }
            }
            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            Column(Modifier.padding(16.dp)) {
                Text("Crash log", style = MaterialTheme.typography.titleSmall)
                TextButton(onClick = { crashLog = readCrash(context) }) {
                    Text("Show last crash")
                }
                if (crashLog.isNotEmpty()) {
                    Text(crashLog, style = MaterialTheme.typography.labelSmall)
                } else {
                    Text(
                        "No crash recorded.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            Spacer(Modifier.height(24.dp))
        }
    }
}

private fun engineLine(): String {
    return try {
        "Engine: " + FussNative.version()
    } catch (e: Throwable) {
        "Engine: not loaded (" + (e.message ?: e.javaClass.simpleName) + ")"
    }
}

private fun engineSelfTest(): String {
    return try {
        FussNative.selfTest().replace("; ", "\n")
    } catch (e: Throwable) {
        "Self-test failed: " + (e.message ?: e.javaClass.simpleName)
    }
}

private fun readCrash(context: android.content.Context): String {
    val text = CrashLog.last(context)
    return text ?: "No crash log found"
}

@Composable
private fun SettingSwitch(label: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, modifier = Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge)
        Switch(checked = checked, onCheckedChange = onChange)
    }
}

@Composable
private fun InfoBlock(title: String, body: String) {
    Column(Modifier.padding(16.dp)) {
        Text(title, style = MaterialTheme.typography.titleSmall)
        Spacer(Modifier.height(4.dp))
        Text(body, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}
