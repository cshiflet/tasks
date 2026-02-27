package org.tasks.desktop.screens

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.outlined.CheckCircle
import androidx.compose.material.icons.outlined.Sync
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import kotlinx.collections.immutable.toImmutableList
import org.tasks.compose.drawer.DrawerAction
import org.tasks.compose.drawer.DrawerItem
import org.tasks.compose.drawer.MenuSearchBar
import org.tasks.compose.drawer.TaskListDrawer
import org.tasks.desktop.DesktopApplication
import org.tasks.desktop.sync.AccountSyncState
import org.tasks.filters.CaldavFilter
import org.tasks.filters.CustomFilter
import org.tasks.filters.FilterProvider
import org.tasks.filters.NavigationDrawerSubheader
import org.tasks.filters.PlaceFilter
import org.tasks.filters.TagFilter
import java.awt.Desktop
import java.net.URI

@Composable
fun SidebarPane(
    application: DesktopApplication,
    modifier: Modifier = Modifier,
) {
    val filters by application.filters.collectAsState()
    val drawerQuery by application.drawerQuery.collectAsState()
    val accountSyncStates by application.syncManager.accountSyncStates.collectAsState()

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
                    application.navigator.navigateToPlaceEdit()
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
        },
        onEditClick = { item ->
            when (val filter = item.filter) {
                is TagFilter -> {
                    filter.tagData.id?.let { tagId ->
                        application.navigator.navigateToTagEdit(tagId)
                    }
                }
                is CaldavFilter -> {
                    filter.calendar.id?.let { listId ->
                        application.navigator.navigateToListEdit(listId = listId)
                    }
                }
                is CustomFilter -> {
                    filter.id?.let { filterId ->
                        application.navigator.navigateToFilterEdit(filterId)
                    }
                }
                is PlaceFilter -> {
                    filter.place.id.let { placeId ->
                        application.navigator.navigateToPlaceEdit(placeId)
                    }
                }
                else -> {
                    // Not editable
                }
            }
        },
        headerTrailing = { header ->
            val isCaldavHeader = header.header.subheaderType == NavigationDrawerSubheader.SubheaderType.CALDAV ||
                    header.header.subheaderType == NavigationDrawerSubheader.SubheaderType.TASKS
            if (isCaldavHeader) {
                when (accountSyncStates[header.header.id]) {
                    AccountSyncState.SYNCING -> SyncingIcon()
                    AccountSyncState.SUCCESS -> SyncSuccessIcon()
                    else -> {} // IDLE and ERROR: no extra icon (ERROR shown by existing SyncProblem)
                }
            }
        },
    )
}

@Composable
private fun SyncingIcon() {
    val infiniteTransition = rememberInfiniteTransition(label = "syncRotation")
    val rotation by infiniteTransition.animateFloat(
        initialValue = 0f,
        targetValue = 360f,
        animationSpec = infiniteRepeatable(
            animation = tween(durationMillis = 1000, easing = LinearEasing),
        ),
        label = "rotation",
    )
    IconButton(onClick = {}, enabled = false) {
        Icon(
            imageVector = Icons.Outlined.Sync,
            contentDescription = "Syncing",
            modifier = Modifier.size(18.dp).rotate(rotation),
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun SyncSuccessIcon() {
    IconButton(onClick = {}, enabled = false) {
        Icon(
            imageVector = Icons.Outlined.CheckCircle,
            contentDescription = "Sync complete",
            modifier = Modifier.size(18.dp),
            tint = Color(0xFF4CAF50),
        )
    }
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
