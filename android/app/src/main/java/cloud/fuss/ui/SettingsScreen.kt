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
import cloud.fuss.app.RecentStore
import cloud.fuss.core.FsRepository
import cloud.fuss.core.FussNative
import kotlinx.coroutines.launch
import androidx.compose.runtime.rememberCoroutineScope

/**
 * Settings and diagnostics. The self-test button runs every engine path on a
 * temporary tree and shows the raw report: no hidden "it works" claims.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(onBack: () -> Unit) {
    val ctx = LocalContext.current
    val prefs = remember { ctx.getSharedPreferences("fuss", 0) }
    var themeDark by remember { mutableStateOf(if (prefs.contains("dark")) prefs.getBoolean("dark", false) else false) }
    var followSystem by remember { mutableStateOf(!prefs.contains("dark")) }
    var dynamic by remember { mutableStateOf(prefs.getBoolean("dynamic", false)) }
    var report by remember { mutableStateOf<String?>(null) }
    var version by remember { mutableStateOf("") }
    val scope = rememberCoroutineScope()

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
        Column(Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState())) {
            SettingsRow("Follow system theme", followSystem) {
                followSystem = it
                if (it) prefs.edit().remove("dark").apply() else prefs.edit().putBoolean("dark", themeDark).apply()
            }
            if (!followSystem) {
                SettingsRow("Dark theme", themeDark) {
                    themeDark = it
                    prefs.edit().putBoolean("dark", it).apply()
                }
            }
            SettingsRow("Dynamic colour (Android 12+)", dynamic) {
                dynamic = it
                prefs.edit().putBoolean("dynamic", it).apply()
            }
            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            Column(Modifier.padding(16.dp)) {
                Text("Privacy", style = MaterialTheme.typography.titleSmall)
                Spacer(Modifier.height(4.dp))
                Text(
                    "Fuss has no network permission, no analytics, no ads and no background services. " +
                        "The only permissions it can request are for reading media files you browse; " +
                        "everything else goes through folders you explicitly grant.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            Column(Modifier.padding(16.dp)) {
                Text("Diagnostics", style = MaterialTheme.typography.titleSmall)
                Spacer(Modifier.height(6.dp))
                Text(
                    "Engine: ${runCatching { FussNative.version() }.getOrDefault("not loaded")}",
                    style = MaterialTheme.typography.bodySmall,
                )
                Spacer(Modifier.height(4.dp))
                TextButton(onClick = { report = FussNative.selfTest() }) {
                    Text("Run engine self-test")
                }
                report?.let {
                    Text(
                        it.replace(";", "\n"),
                        style = MaterialTheme.typography.bodySmall,
                        color = if (it.contains("FAIL")) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            Column(Modifier.padding(16.dp)) {
                Text("Clear history", style = MaterialTheme.typography.titleSmall)
                TextButton(onClick = { RecentStore(ctx).clearRecent() }) { Text("Clear recent folders") }
            }
            Spacer(Modifier.height(24.dp))
        }
    }
}

@Composable
private fun SettingsRow(label: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, modifier = Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge)
        Switch(checked = checked, onCheckedChange = onChange)
    }
}
