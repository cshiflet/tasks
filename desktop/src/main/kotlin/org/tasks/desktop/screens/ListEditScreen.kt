package org.tasks.desktop.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.tasks.data.UUIDHelper
import org.tasks.data.entity.CaldavAccount
import org.tasks.data.entity.CaldavCalendar
import org.tasks.desktop.DesktopApplication
import org.tasks.desktop.sync.DesktopCaldavClient
import org.tasks.desktop.sync.DesktopEtebaseClient

private val LIST_COLORS = listOf(
    0 to "None",
    0xFFE57373.toInt() to "Red",
    0xFFFF8A65.toInt() to "Orange",
    0xFFFFD54F.toInt() to "Yellow",
    0xFF81C784.toInt() to "Green",
    0xFF4FC3F7.toInt() to "Light Blue",
    0xFF64B5F6.toInt() to "Blue",
    0xFF9575CD.toInt() to "Purple",
    0xFFF06292.toInt() to "Pink",
    0xFFA1887F.toInt() to "Brown",
    0xFF90A4AE.toInt() to "Gray",
)

@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
fun ListEditScreen(
    listId: Long?,
    accountId: Long?,
    application: DesktopApplication,
    onNavigateBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val scope = rememberCoroutineScope()
    val isNew = listId == null

    var calendar by remember { mutableStateOf<CaldavCalendar?>(null) }
    var name by remember { mutableStateOf("") }
    var selectedColor by remember { mutableStateOf(0) }
    var showDeleteDialog by remember { mutableStateOf(false) }
    var isLoading by remember { mutableStateOf(!isNew) }
    var errorMessage by remember { mutableStateOf<String?>(null) }
    var isSaving by remember { mutableStateOf(false) }

    LaunchedEffect(listId) {
        if (listId != null) {
            withContext(Dispatchers.IO) {
                val loaded = application.caldavDao.getCalendarById(listId)
                calendar = loaded
                loaded?.let {
                    name = it.name ?: ""
                    selectedColor = it.color
                }
            }
            isLoading = false
        }
    }

    fun saveList() {
        if (name.isBlank() || isSaving) return
        isSaving = true
        errorMessage = null
        scope.launch(Dispatchers.IO) {
            try {
                if (isNew) {
                    val account = accountId?.let { application.caldavDao.getAccount(it) }
                    val trimmedName = name.trim()
                    val calendarUrl: String? = when (account?.accountType) {
                        CaldavAccount.TYPE_CALDAV -> {
                            // Create the calendar on the CalDAV server first, then persist the URL.
                            val serverUrl = account.url ?: error("Account has no server URL")
                            val username = account.username ?: error("Account has no username")
                            val password = account.password ?: error("Account has no password")
                            val client = DesktopCaldavClient.forAccount(serverUrl, username, password)
                            client.makeCollection(trimmedName, selectedColor)
                        }
                        CaldavAccount.TYPE_ETEBASE -> {
                            // Create the collection on the EteSync server; the returned UID is the URL.
                            val serverUrl = account.url ?: error("Account has no server URL")
                            val username = account.username ?: error("Account has no username")
                            val session = account.password ?: error("Account has no session")
                            val client = DesktopEtebaseClient.forAccount(serverUrl, username, session, application.caldavDao)
                            client.makeCollection(trimmedName, selectedColor)
                        }
                        else -> null
                    }
                    application.caldavDao.insert(
                        CaldavCalendar(
                            account = account?.uuid,
                            uuid = UUIDHelper.newUUID(),
                            name = trimmedName,
                            color = selectedColor,
                            url = calendarUrl,
                        )
                    )
                } else {
                    val cal = calendar ?: return@launch
                    val account = cal.account?.let { application.caldavDao.getAccountByUuid(it) }
                    when (account?.accountType) {
                        CaldavAccount.TYPE_CALDAV -> {
                            // Update display name/color on the CalDAV server.
                            if (!cal.url.isNullOrBlank()) {
                                val serverUrl = account.url ?: error("Account has no server URL")
                                val username = account.username ?: error("Account has no username")
                                val password = account.password ?: error("Account has no password")
                                val client = DesktopCaldavClient.forAccount(serverUrl, username, password)
                                client.updateCollection(cal.url!!, name.trim(), selectedColor)
                            }
                        }
                        CaldavAccount.TYPE_ETEBASE -> {
                            // Update collection metadata on the EteSync server.
                            val serverUrl = account.url ?: error("Account has no server URL")
                            val username = account.username ?: error("Account has no username")
                            val session = account.password ?: error("Account has no session")
                            val client = DesktopEtebaseClient.forAccount(serverUrl, username, session, application.caldavDao)
                            client.updateCollection(cal, name.trim(), selectedColor)
                        }
                    }
                    application.caldavDao.update(
                        cal.copy(name = name.trim(), color = selectedColor)
                    )
                }
                withContext(Dispatchers.Main) { onNavigateBack() }
            } catch (e: Exception) {
                withContext(Dispatchers.Main) {
                    errorMessage = e.message ?: "Failed to save list"
                    isSaving = false
                }
            }
        }
    }

    fun deleteList() {
        scope.launch(Dispatchers.IO) {
            calendar?.let { cal ->
                // Delete from the EteSync server before removing locally.
                val account = cal.account?.let { application.caldavDao.getAccountByUuid(it) }
                if (account?.accountType == CaldavAccount.TYPE_ETEBASE && !cal.url.isNullOrBlank()) {
                    try {
                        val serverUrl = account.url ?: error("Account has no server URL")
                        val username = account.username ?: error("Account has no username")
                        val session = account.password ?: error("Account has no session")
                        val client = DesktopEtebaseClient.forAccount(serverUrl, username, session, application.caldavDao)
                        client.deleteCollection(cal)
                    } catch (e: Exception) {
                        // Log but don't block local deletion
                    }
                }
                application.deletionDao.delete(cal) { /* no local cleanup needed on desktop */ }
            }
            withContext(Dispatchers.Main) {
                onNavigateBack()
            }
        }
    }

    errorMessage?.let { msg ->
        AlertDialog(
            onDismissRequest = { errorMessage = null },
            title = { Text("Error") },
            text = { Text(msg) },
            confirmButton = {
                TextButton(onClick = { errorMessage = null }) { Text("OK") }
            },
        )
    }

    if (showDeleteDialog) {
        AlertDialog(
            onDismissRequest = { showDeleteDialog = false },
            title = { Text("Delete list?") },
            text = { Text("This will delete the list \"$name\" and all tasks in it.") },
            confirmButton = {
                TextButton(
                    onClick = {
                        showDeleteDialog = false
                        deleteList()
                    }
                ) {
                    Text("Delete", color = MaterialTheme.colorScheme.error)
                }
            },
            dismissButton = {
                TextButton(onClick = { showDeleteDialog = false }) {
                    Text("Cancel")
                }
            }
        )
    }

    Scaffold(
        modifier = modifier.fillMaxSize(),
        topBar = {
            TopAppBar(
                title = { Text(if (isNew) "New list" else "Edit list") },
                navigationIcon = {
                    IconButton(onClick = onNavigateBack) {
                        Icon(
                            imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = "Back",
                        )
                    }
                },
                actions = {
                    if (!isNew && calendar != null) {
                        IconButton(onClick = { showDeleteDialog = true }) {
                            Icon(
                                imageVector = Icons.Default.Delete,
                                contentDescription = "Delete",
                                tint = MaterialTheme.colorScheme.error,
                            )
                        }
                    }
                    IconButton(
                        onClick = { saveList() },
                        enabled = name.isNotBlank() && !isSaving,
                    ) {
                        Icon(
                            imageVector = Icons.Default.Check,
                            contentDescription = "Save",
                        )
                    }
                }
            )
        }
    ) { paddingValues ->
        if (isLoading) {
            Box(
                modifier = Modifier.fillMaxSize().padding(paddingValues),
                contentAlignment = Alignment.Center,
            ) {
                Text("Loading...")
            }
        } else {
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(paddingValues)
                    .padding(16.dp)
                    .verticalScroll(rememberScrollState())
            ) {
                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it },
                    label = { Text("List name") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )

                Spacer(modifier = Modifier.height(24.dp))

                Text(
                    text = "Color",
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.onSurface,
                )

                Spacer(modifier = Modifier.height(12.dp))

                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    LIST_COLORS.forEach { (colorValue, colorName) ->
                        ColorOption(
                            color = colorValue,
                            name = colorName,
                            isSelected = selectedColor == colorValue,
                            onClick = { selectedColor = colorValue },
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun ColorOption(
    color: Int,
    name: String,
    isSelected: Boolean,
    onClick: () -> Unit,
) {
    Column(
        horizontalAlignment = Alignment.CenterHorizontally,
        modifier = Modifier.clickable(onClick = onClick),
    ) {
        Box(
            modifier = Modifier
                .size(40.dp)
                .clip(CircleShape)
                .background(
                    if (color == 0) MaterialTheme.colorScheme.surfaceVariant
                    else Color(color)
                )
                .then(
                    if (isSelected) Modifier.border(
                        width = 3.dp,
                        color = MaterialTheme.colorScheme.primary,
                        shape = CircleShape,
                    ) else Modifier
                ),
            contentAlignment = Alignment.Center,
        ) {
            if (isSelected) {
                Icon(
                    imageVector = Icons.Default.Check,
                    contentDescription = "Selected",
                    tint = if (color == 0) MaterialTheme.colorScheme.onSurfaceVariant else Color.White,
                    modifier = Modifier.size(20.dp),
                )
            }
        }
        Spacer(modifier = Modifier.height(4.dp))
        Text(
            text = name,
            style = MaterialTheme.typography.bodySmall,
            color = if (isSelected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
