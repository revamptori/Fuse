package cloud.fuss.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material3.ExperimentalMaterial3Api
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
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import cloud.fuss.core.FsRepository
import cloud.fuss.core.FussException
import cloud.fuss.core.SearchResults
import kotlinx.coroutines.delay
import java.io.File

/**
 * Incremental search inside [root]. Typing debounces 220 ms, then the engine
 * walks the tree breadth-first. A new query cancels the previous scan through
 * its native cancel handle (released by the repository), so results never mix.
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
            TopAppBar(
                title = {
                    OutlinedTextField(
                        value = query,
                        onValueChange = { query = it },
                        placeholder = { Text("Search in ${File(root).name.ifEmpty { root }}") },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                        leadingIcon = { Icon(Icons.Outlined.Search, contentDescription = null) },
                        trailingIcon = {
                            if (includeHidden) Text("•hidden", style = MaterialTheme.typography.labelSmall)
                        },
                    )
                },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Outlined.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            if (scanning) BusyBanner("Searching…")
            val r = results
            when {
                error != null -> EmptyState(title = error!!)
                query.length < 2 && query.isNotEmpty() -> EmptyState(title = "Keep typing", subtitle = "At least two characters are needed.")
                r == null -> EmptyState(title = "Search ${File(root).name.ifEmpty { root }}", subtitle = "Substring match, or a glob like *.jpg.")
                r.hits.isEmpty() -> EmptyState(title = "No matches", subtitle = "Scanned ${r.scanned} items in ${r.elapsedMs} ms.")
                else -> LazyColumn(Modifier.fillMaxSize()) {
                    item {
                        Text(
                            "${r.hits.size} results${if (r.truncated) " (truncated)" else ""} · ${r.elapsedMs} ms",
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
                                if (hit.entry.isDir) onOpenPath(hit.path) else onOpenPath(hit.path.substringBeforeLast('/'))
                            },
                        )
                    }
                }
            }
        }
    }

    LaunchedEffect(query, includeHidden) {
        if (query.length < 2) {
            results = null
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
            // Coroutine cancellation while a newer query replaces this one.
        }
        scanning = false
    }
}
