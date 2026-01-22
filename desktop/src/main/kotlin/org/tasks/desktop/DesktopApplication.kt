package org.tasks.desktop

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.datastore.preferences.core.booleanPreferencesKey
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import org.tasks.compose.drawer.DrawerItem
import org.tasks.desktop.di.DesktopContainer
import org.tasks.filters.Filter
import org.tasks.filters.FilterListItem
import org.tasks.filters.MyTasksFilter
import org.tasks.filters.NavigationDrawerSubheader

class DesktopApplication(
    val container: DesktopContainer = DesktopContainer.getInstance()
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)

    // Use hardcoded title since MyTasksFilter.create() is suspend
    var currentFilter: Filter by mutableStateOf(MyTasksFilter(title = "My Tasks"))
        private set

    private val _filters = MutableStateFlow<List<DrawerItem>>(emptyList())
    val filters: StateFlow<List<DrawerItem>> = _filters.asStateFlow()

    private val _drawerQuery = MutableStateFlow("")
    val drawerQuery: StateFlow<String> = _drawerQuery.asStateFlow()

    val navigator get() = container.navigator
    val taskDao get() = container.taskDao
    val caldavDao get() = container.caldavDao
    val tagDataDao get() = container.tagDataDao
    val tagDao get() = container.tagDao
    val alarmDao get() = container.alarmDao
    val filterDao get() = container.filterDao
    val locationDao get() = container.locationDao
    val deletionDao get() = container.deletionDao
    val tasksPreferences get() = container.tasksPreferences

    init {
        loadFilters()
    }

    fun loadFilters() {
        scope.launch(Dispatchers.IO) {
            val items = container.filterProvider.drawerItems()
            _filters.value = items.toDrawerItems(currentFilter, _drawerQuery.value)
        }
    }

    fun selectFilter(filter: Filter) {
        currentFilter = filter
        loadFilters()
    }

    fun toggleHeader(header: NavigationDrawerSubheader) {
        scope.launch(Dispatchers.IO) {
            when (header.subheaderType) {
                NavigationDrawerSubheader.SubheaderType.PREFERENCE -> {
                    val key = booleanPreferencesKey(header.id)
                    val currentValue = container.tasksPreferences.get(key, false)
                    container.tasksPreferences.set(key, !currentValue)
                }
                NavigationDrawerSubheader.SubheaderType.CALDAV,
                NavigationDrawerSubheader.SubheaderType.TASKS -> {
                    val accountId = header.id.toLongOrNull() ?: return@launch
                    val account = container.caldavDao.getAccount(accountId) ?: return@launch
                    container.caldavDao.update(account.copy(isCollapsed = !account.isCollapsed))
                }
            }
            loadFilters()
        }
    }

    fun updateDrawerQuery(query: String) {
        _drawerQuery.value = query
        loadFilters()
    }

    private fun List<FilterListItem>.toDrawerItems(
        selected: Filter,
        query: String
    ): List<DrawerItem> {
        val queryLower = query.lowercase()
        return mapNotNull { item ->
            when (item) {
                is Filter -> {
                    if (query.isBlank() || item.title?.lowercase()?.contains(queryLower) == true) {
                        DrawerItem.Filter(
                            title = item.title ?: "",
                            icon = item.icon,
                            color = item.tint,
                            count = item.count,
                            shareCount = 0,
                            selected = item == selected,
                            filter = item,
                        )
                    } else null
                }
                is NavigationDrawerSubheader -> {
                    if (query.isBlank()) {
                        DrawerItem.Header(
                            title = item.title ?: "",
                            collapsed = item.isCollapsed,
                            hasError = item.error,
                            canAdd = item.addIntentRc != 0,
                            header = item,
                        )
                    } else null
                }
                else -> null
            }
        }
    }
}
