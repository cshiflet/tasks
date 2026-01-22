package org.tasks.desktop.screens

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlinx.collections.immutable.toImmutableList
import org.tasks.compose.drawer.DrawerAction
import org.tasks.compose.drawer.DrawerItem
import org.tasks.compose.drawer.MenuSearchBar
import org.tasks.compose.drawer.TaskListDrawer
import org.tasks.desktop.DesktopApplication
import org.tasks.filters.FilterProvider
import java.awt.Desktop
import java.net.URI

@Composable
fun SidebarPane(
    application: DesktopApplication,
    modifier: Modifier = Modifier,
) {
    val filters by application.filters.collectAsState()
    val drawerQuery by application.drawerQuery.collectAsState()

    TaskListDrawer(
        arrangement = Arrangement.spacedBy(0.dp),
        filters = filters.toImmutableList(),
        onClick = { item ->
            when (item) {
                is DrawerItem.Filter -> {
                    application.selectFilter(item.filter)
                }
                is DrawerItem.Header -> {
                    application.toggleHeader(item.header)
                }
            }
        },
        onAddClick = { header ->
            when (header.header.addIntentRc) {
                FilterProvider.REQUEST_NEW_LIST -> {
                    application.navigator.navigateToListEdit(accountId = header.header.id.toLongOrNull())
                }
                FilterProvider.REQUEST_NEW_TAGS -> {
                    application.navigator.navigateToTagEdit()
                }
                FilterProvider.REQUEST_NEW_PLACE -> {
                    // Location creation not supported on desktop yet
                }
                FilterProvider.REQUEST_NEW_FILTER -> {
                    application.navigator.navigateToFilterEdit()
                }
            }
        },
        onErrorClick = {
            application.navigator.navigateToSettings()
        },
        searchBar = {
            MenuSearchBar(
                begForMoney = false,
                onDrawerAction = { action ->
                    when (action) {
                        DrawerAction.PURCHASE -> {
                            openInBrowser("https://tasks.org/subscribe")
                        }
                        DrawerAction.HELP_AND_FEEDBACK -> {
                            openInBrowser("https://tasks.org/help")
                        }
                    }
                },
                query = drawerQuery,
                onQueryChange = { application.updateDrawerQuery(it) }
            )
        }
    )
}

private fun openInBrowser(url: String) {
    try {
        if (Desktop.isDesktopSupported()) {
            Desktop.getDesktop().browse(URI(url))
        }
    } catch (e: Exception) {
        // Ignore
    }
}
