package org.tasks.desktop.navigation

import org.tasks.filters.Filter

sealed class Screen {
    data object TaskList : Screen()
    data class TaskEdit(val taskId: Long? = null, val filter: Filter? = null) : Screen()
    data object Settings : Screen()
    data object AccountSetup : Screen()
    data class AccountEdit(val accountId: Long) : Screen()
    data class ListEdit(val listId: Long? = null, val accountId: Long? = null) : Screen()
    data class TagEdit(val tagId: Long? = null) : Screen()
    data class FilterEdit(val filterId: Long? = null) : Screen()
    data class PlaceEdit(val placeId: Long? = null) : Screen()
}
