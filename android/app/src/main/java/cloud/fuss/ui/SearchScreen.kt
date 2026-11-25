package cloud.fuss.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import cloud.fuss.core.FsRepository
import cloud.fuss.core.FussException
import cloud.fuss.core.SearchResults
import kotlinx.coroutines.delay
import java.io.File

/**
 * Incremental search inside [root]. Typing is debounced by 220 ms; each new
 * query replaces the previous composition effect, which cancels the in-flight
 * engine scan through its cancel token, so stale results can never appear.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SearchScreen(root: String, onBack: () -> Unit, onOpenPath: (String) -> Unit) {
    val repo = remember { FsRepository() }
    var query by remember { mutableStateOf("") }
    var includeHidden by remember { mutableStateOf(false) }
    var results by remember { mutableStateOf<SearchResults?>(null) }
    var scanning by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }

    Scaffold(
        topBar = {
            Column {
                TopAppBar(
                    title = {
                        OutlinedTextField(
                            value = query,
                            onValueChange = { query = it },
                            placeholder = { Text("Search ${File(root).name.ifEmpty { root }}") },
                            singleLine = true,
                            modifier = Modifier.fillMaxWidth(),
                            leadingIcon = { Icon(Icons.Outlined.Search, contentDescription = null) },
                        )
                    },
                    navigationIcon = {
                        IconButton(onClick = onBack) {
                            Icon(Icons.AutoMirrored.Outlined.ArrowBack, contentDescription = "Back")
                        }
                    },
                )
                Row(Modifier.padding(horizontal = 16.dp, vertical = 2.dp), verticalAlignment = Alignment.CenterVertically) {
                    FilterChip(
                        selected = includeHidden,
                        onClick = { includeHidden = !includeHidden },
                        label = { Text("Hidden files") },
                    )
                    Spacer(Modifier.width(8.dp))
                    Text(
                        "Substring match or glob, e.g. *.jpg",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            if (scanning) BusyBanner("Searching…")
            val r = results
            when {
                error != null -> EmptyState(title = error!!)
                query.isEmpty() -> EmptyState(
                    title = "Search ${File(root).name.ifEmpty { root }}",
                    subtitle = "Type at least two characters. Results stream in from the engine's breadth-first walk.",
                )
                query.length < 2 -> EmptyState(title = "Keep typing", subtitle = "At least two characters are needed.")
                r == null -> LoadingState()
                r.hits.isEmpty() -> EmptyState(
                    title = "No matches",
                    subtitle = "Scanned ${r.scanned} items in ${r.elapsedMs} ms.",
                )
                else -> LazyColumn(Modifier.fillMaxSize()) {
                    item {
                        Text(
                            "${r.hits.size} results${if (r.truncated) " (truncated)" else ""} · " +
                                "scanned ${r.scanned} in ${r.elapsedMs} ms",
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(16.dp),
                        )
                    }
                    items(r.hits, key = { it.path }) { hit ->
                        TwoLineRow(
                            primary = hit.entry.name,
                            secondary = hit.path.substringBeforeLast('/'),
                            leading = {
                                Icon(
                                    FileIcons.icon(hit.entry.kind),
                                    contentDescription = null,
                                    modifier = Modifier.size(22.dp),
                                    tint = FileIcons.tint(hit.entry.kind),
                                )
                            },
                            onClick = {
                                if (hit.entry.isDir) onOpenPath(hit.path)
                                else onOpenPath(hit.path.substringBeforeLast('/'))
                            },
                        )
                    }
                }
            }
        }
    }

    LaunchedEffect(query, includeHidden, root) {
        if (query.length < 2) {
            results = null
            scanning = false
            return@LaunchedEffect
        }
        delay(220)
        scanning = true
        error = null
        try {
            results = repo.search(root, query, includeHidden)
        } catch (e: FussException) {
            if (!e.isCancellation) error = e.userMessage()
        } catch (_: Exception) {
            // A newer query cancelled this one.
        } finally {
            scanning = false
        }
    }
}
