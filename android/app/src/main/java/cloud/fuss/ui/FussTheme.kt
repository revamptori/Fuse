package cloud.fuss.ui

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext

/**
 * Material 3 theme with a deliberately muted palette: one accent, neutral
 * surfaces, no elevation shadows or gradients. Follows the system light/dark
 * setting (or an explicit override from settings) and supports Android 12+
 * dynamic colour only when the user opts in.
 */
private val LightColors = lightColorScheme(
    primary = Color(0xFF2F6D5B),
    onPrimary = Color.White,
    primaryContainer = Color(0xFFB2F0D9),
    onPrimaryContainer = Color(0xFF002117),
    secondary = Color(0xFF4D6359),
    surface = Color(0xFFFBFDFA),
    background = Color(0xFFF7FAF7),
    surfaceVariant = Color(0xFFDCE5DF),
    onSurface = Color(0xFF191C1A),
    onSurfaceVariant = Color(0xFF404944),
    outline = Color(0xFF707974),
    error = Color(0xFFBA1A1A),
)

private val DarkColors = darkColorScheme(
    primary = Color(0xFF96D5BD),
    onPrimary = Color(0xFF003829),
    primaryContainer = Color(0xFF13513F),
    onPrimaryContainer = Color(0xFFB2F0D9),
    secondary = Color(0xFFB4CCC0),
    surface = Color(0xFF101412),
    background = Color(0xFF0B0F0D),
    surfaceVariant = Color(0xFF404944),
    onSurface = Color(0xFFE1E3DF),
    onSurfaceVariant = Color(0xFFBFC9C2),
    outline = Color(0xFF89938D),
    error = Color(0xFFFFB4AB),
)

@Composable
fun FussTheme(
    dark: Boolean? = null,
    dynamicColor: Boolean = false,
    content: @Composable () -> Unit,
) {
    val darkTheme = dark ?: isSystemInDarkTheme()
    val context = LocalContext.current
    val scheme = when {
        dynamicColor && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S -> {
            if (darkTheme) {
                androidx.compose.material3.dynamicDarkColorScheme(context)
            } else {
                androidx.compose.material3.dynamicLightColorScheme(context)
            }
        }
        darkTheme -> DarkColors
        else -> LightColors
    }
    MaterialTheme(colorScheme = scheme, content = content)
}
