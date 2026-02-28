package org.tasks.desktop.screens

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
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
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.tasks.data.entity.CaldavAccount
import org.tasks.desktop.DesktopApplication

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AccountEditScreen(
    accountId: Long,
    application: DesktopApplication,
    onNavigateBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val scope = rememberCoroutineScope()

    var account by remember { mutableStateOf<CaldavAccount?>(null) }
    var name by remember { mutableStateOf("") }
    var url by remember { mutableStateOf("") }
    var username by remember { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    var showDeleteDialog by remember { mutableStateOf(false) }
    var isLoading by remember { mutableStateOf(true) }

    LaunchedEffect(accountId) {
        withContext(Dispatchers.IO) {
            val loaded = application.caldavDao.getAccount(accountId)
            account = loaded
            loaded?.let {
                name = it.name ?: ""
                url = it.url ?: ""
                username = it.username ?: ""
                // EteSync stores a session token (not password) — never display it.
                password = if (it.accountType == CaldavAccount.TYPE_ETEBASE) "" else it.password ?: ""
            }
        }
        isLoading = false
    }

    fun saveAccount() {
        scope.launch(Dispatchers.IO) {
            account?.let { acc ->
                application.caldavDao.update(
                    acc.copy(
                        name = name.trim().ifBlank { "CalDAV" },
                        url = url.trim(),
                        username = username.trim(),
                        // For EteSync, preserve the existing session token — never overwrite it.
                        password = if (acc.accountType == CaldavAccount.TYPE_ETEBASE) acc.password else password,
                    )
                )
            }
            withContext(Dispatchers.Main) {
                onNavigateBack()
            }
        }
    }

    fun deleteAccount() {
        scope.launch(Dispatchers.IO) {
            account?.let {
                application.deletionDao.delete(it) { /* no local cleanup needed on desktop */ }
            }
            withContext(Dispatchers.Main) {
                onNavigateBack()
            }
        }
    }

    if (showDeleteDialog) {
        AlertDialog(
            onDismissRequest = { showDeleteDialog = false },
            title = { Text("Remove account?") },
            text = { Text("This will remove the account \"$name\" and all its tasks.") },
            confirmButton = {
                TextButton(
                    onClick = {
                        showDeleteDialog = false
                        deleteAccount()
                    }
                ) {
                    Text("Remove", color = MaterialTheme.colorScheme.error)
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
                title = { Text("Edit account") },
                navigationIcon = {
                    IconButton(onClick = onNavigateBack) {
                        Icon(
                            imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = "Back",
                        )
                    }
                },
                actions = {
                    if (account != null) {
                        IconButton(onClick = { showDeleteDialog = true }) {
                            Icon(
                                imageVector = Icons.Default.Delete,
                                contentDescription = "Remove account",
                                tint = MaterialTheme.colorScheme.error,
                            )
                        }
                    }
                    IconButton(
                        onClick = { saveAccount() },
                        enabled = !isLoading && url.isNotBlank() && username.isNotBlank(),
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
                    label = { Text("Account name") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )

                Spacer(modifier = Modifier.height(16.dp))

                OutlinedTextField(
                    value = url,
                    onValueChange = { url = it },
                    label = { Text("Server URL") },
                    placeholder = { Text("https://caldav.example.com") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )

                Spacer(modifier = Modifier.height(16.dp))

                OutlinedTextField(
                    value = username,
                    onValueChange = { username = it },
                    label = { Text("Username") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )

                Spacer(modifier = Modifier.height(16.dp))

                OutlinedTextField(
                    value = password,
                    onValueChange = { password = it },
                    label = { Text("Password") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                    visualTransformation = PasswordVisualTransformation(),
                )
            }
        }
    }
}
