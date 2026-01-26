package org.tasks.desktop.navigation

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

class Navigator {
    private val _currentScreen = MutableStateFlow<Screen>(Screen.TaskList)
    val currentScreen: StateFlow<Screen> = _currentScreen.asStateFlow()

    private val backStack = mutableListOf<Screen>()

    fun navigate(screen: Screen) {
        backStack.add(_currentScreen.value)
        _currentScreen.value = screen
    }

    fun goBack(): Boolean {
        return if (backStack.isNotEmpty()) {
            _currentScreen.value = backStack.removeLast()
            true
        } else {
            false
        }
    }

    fun navigateToTaskList() {
        backStack.clear()
        _currentScreen.value = Screen.TaskList
    }

    fun navigateToTaskEdit(taskId: Long? = null) {
        navigate(Screen.TaskEdit(taskId))
    }

    fun navigateToSettings() {
        navigate(Screen.Settings)
    }

    fun navigateToAccountSetup() {
        navigate(Screen.AccountSetup)
    }

    fun navigateToAccountEdit(accountId: Long) {
        navigate(Screen.AccountEdit(accountId))
    }

    fun navigateToListEdit(listId: Long? = null, accountId: Long? = null) {
        navigate(Screen.ListEdit(listId, accountId))
    }

    fun navigateToTagEdit(tagId: Long? = null) {
        navigate(Screen.TagEdit(tagId))
    }

    fun navigateToFilterEdit(filterId: Long? = null) {
        navigate(Screen.FilterEdit(filterId))
    }

    fun navigateToPlaceEdit(placeId: Long? = null) {
        navigate(Screen.PlaceEdit(placeId))
    }
}
