package org.tasks.desktop.screens

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.CalendarMonth
import androidx.compose.material.icons.filled.Cloud
import androidx.compose.material.icons.filled.Security
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.tasks.data.entity.CaldavAccount
import org.tasks.desktop.DesktopApplication
import org.tasks.desktop.sync.DesktopEtebaseClient
import org.tasks.desktop.sync.authorizeGoogleTasks

enum class AccountType {
    CALDAV,
    GOOGLE_TASKS,
    ETEBASE,
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AccountSetupScreen(
    application: DesktopApplication,
    onNavigateBack: () -> Unit,
    onAccountCreated: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var selectedType by remember { mutableStateOf<AccountType?>(null) }

    Scaffold(
        modifier = modifier.fillMaxSize(),
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        text = if (selectedType == null) "Add Account" else getAccountTypeTitle(selectedType!!),
                        style = MaterialTheme.typography.titleLarge,
                    )
                },
                navigationIcon = {
                    IconButton(onClick = {
                        if (selectedType != null) {
                            selectedType = null
                        } else {
                            onNavigateBack()
                        }
                    }) {
                        Icon(
                            imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = "Back",
                        )
                    }
                },
            )
        }
    ) { paddingValues ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(paddingValues)
                .verticalScroll(rememberScrollState())
                .padding(16.dp)
        ) {
            if (selectedType == null) {
                AccountTypeSelector(
                    onTypeSelected = { selectedType = it }
                )
            } else {
                when (selectedType) {
                    AccountType.CALDAV -> CaldavAccountForm(
                        application = application,
                        onAccountCreated = onAccountCreated,
                    )
                    AccountType.GOOGLE_TASKS -> GoogleTasksAccountForm(
                        application = application,
                        onAccountCreated = onAccountCreated,
                    )
                    AccountType.ETEBASE -> EtebaseAccountForm(
                        application = application,
                        onAccountCreated = onAccountCreated,
                    )
                    null -> {}
                }
            }
        }
    }
}

@Composable
private fun AccountTypeSelector(
    onTypeSelected: (AccountType) -> Unit,
) {
    Text(
        text = "Choose account type",
        style = MaterialTheme.typography.titleMedium,
        modifier = Modifier.padding(bottom = 16.dp)
    )

    AccountTypeCard(
        icon = Icons.Default.CalendarMonth,
        title = "CalDAV",
        description = "Sync with CalDAV servers like Nextcloud, Radicale, or any CalDAV-compatible service",
        onClick = { onTypeSelected(AccountType.CALDAV) }
    )

    Spacer(modifier = Modifier.height(12.dp))

    AccountTypeCard(
        icon = Icons.Default.Cloud,
        title = "Google Tasks",
        description = "Sync with Google Tasks using your Google account",
        onClick = { onTypeSelected(AccountType.GOOGLE_TASKS) }
    )

    Spacer(modifier = Modifier.height(12.dp))

    AccountTypeCard(
        icon = Icons.Default.Security,
        title = "EteSync",
        description = "Sync with EteSync — an end-to-end encrypted task synchronization service",
        onClick = { onTypeSelected(AccountType.ETEBASE) }
    )
}

@Composable
private fun AccountTypeCard(
    icon: ImageVector,
    title: String,
    description: String,
    onClick: () -> Unit,
) {
    Card(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceVariant,
        )
    ) {
        Row(
            modifier = Modifier.padding(16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                imageVector = icon,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.primary,
            )
            Spacer(modifier = Modifier.width(16.dp))
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = title,
                    style = MaterialTheme.typography.titleMedium,
                )
                Text(
                    text = description,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun CaldavAccountForm(
    application: DesktopApplication,
    onAccountCreated: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    var name by remember { mutableStateOf("") }
    var url by remember { mutableStateOf("") }
    var username by remember { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    var isLoading by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }

    Column {
        OutlinedTextField(
            value = name,
            onValueChange = { name = it },
            label = { Text("Account name") },
            placeholder = { Text("My CalDAV Server") },
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

        if (error != null) {
            Spacer(modifier = Modifier.height(16.dp))
            Text(
                text = error!!,
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall,
            )
        }

        Spacer(modifier = Modifier.height(24.dp))

        Button(
            onClick = {
                if (url.isBlank() || username.isBlank() || password.isBlank()) {
                    error = "Please fill in all fields"
                    return@Button
                }
                isLoading = true
                error = null
                scope.launch(Dispatchers.IO) {
                    try {
                        val account = CaldavAccount(
                            name = name.ifBlank { "CalDAV" },
                            url = url,
                            username = username,
                            password = password, // TODO: encrypt
                            accountType = CaldavAccount.TYPE_CALDAV,
                        )
                        application.caldavDao.insert(account)
                        withContext(Dispatchers.Main) {
                            onAccountCreated()
                        }
                    } catch (e: Exception) {
                        withContext(Dispatchers.Main) {
                            error = e.message ?: "Failed to create account"
                            isLoading = false
                        }
                    }
                }
            },
            modifier = Modifier.fillMaxWidth(),
            enabled = !isLoading,
        ) {
            if (isLoading) {
                CircularProgressIndicator(
                    modifier = Modifier.height(20.dp).width(20.dp),
                    strokeWidth = 2.dp,
                )
            } else {
                Text("Add Account")
            }
        }
    }
}

@Composable
private fun GoogleTasksAccountForm(
    application: DesktopApplication,
    onAccountCreated: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    var name by remember { mutableStateOf("") }
    var clientId by remember { mutableStateOf("") }
    var clientSecret by remember { mutableStateOf("") }
    var isLoading by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }

    Column {
        Text(
            text = "You need a Google OAuth 2.0 Desktop App client ID and secret.",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )

        Spacer(modifier = Modifier.height(4.dp))

        Text(
            text = "Create one at console.cloud.google.com → APIs & Services → Credentials → " +
                "Create Credentials → OAuth client ID → Desktop app. " +
                "Enable the Google Tasks API for your project first.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )

        Spacer(modifier = Modifier.height(16.dp))

        OutlinedTextField(
            value = name,
            onValueChange = { name = it },
            label = { Text("Account name (optional)") },
            placeholder = { Text("My Google Tasks") },
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
        )

        Spacer(modifier = Modifier.height(16.dp))

        OutlinedTextField(
            value = clientId,
            onValueChange = { clientId = it },
            label = { Text("Client ID") },
            placeholder = { Text("1234567890-abc.apps.googleusercontent.com") },
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
        )

        Spacer(modifier = Modifier.height(16.dp))

        OutlinedTextField(
            value = clientSecret,
            onValueChange = { clientSecret = it },
            label = { Text("Client Secret") },
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
            visualTransformation = PasswordVisualTransformation(),
        )

        if (error != null) {
            Spacer(modifier = Modifier.height(16.dp))
            Text(
                text = error!!,
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall,
            )
        }

        Spacer(modifier = Modifier.height(8.dp))

        Text(
            text = "Clicking \"Sign in\" will open your browser. After authorizing, return here — " +
                "the account will be created automatically.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )

        Spacer(modifier = Modifier.height(16.dp))

        Button(
            onClick = {
                if (clientId.isBlank()) {
                    error = "Please enter your Client ID"
                    return@Button
                }
                isLoading = true
                error = null
                scope.launch(Dispatchers.IO) {
                    try {
                        val result = authorizeGoogleTasks(
                            clientId = clientId.trim(),
                            clientSecret = clientSecret.trim(),
                        )
                        val account = CaldavAccount(
                            name = name.ifBlank { result.email },
                            username = result.email,
                            // url stores "clientId:::clientSecret" for token refresh
                            url = "${clientId.trim()}:::${clientSecret.trim()}",
                            password = result.refreshToken,
                            accountType = CaldavAccount.TYPE_GOOGLE_TASKS,
                        )
                        application.caldavDao.insert(account)
                        withContext(Dispatchers.Main) {
                            onAccountCreated()
                        }
                    } catch (e: Exception) {
                        withContext(Dispatchers.Main) {
                            error = e.message ?: "Failed to authenticate with Google"
                            isLoading = false
                        }
                    }
                }
            },
            modifier = Modifier.fillMaxWidth(),
            enabled = !isLoading && clientId.isNotBlank(),
        ) {
            if (isLoading) {
                CircularProgressIndicator(
                    modifier = Modifier.height(20.dp).width(20.dp),
                    strokeWidth = 2.dp,
                )
            } else {
                Text("Sign in with Google")
            }
        }
    }
}

@Composable
private fun EtebaseAccountForm(
    application: DesktopApplication,
    onAccountCreated: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    var name by remember { mutableStateOf("") }
    var serverUrl by remember { mutableStateOf("https://api.etebase.com/") }
    var username by remember { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    var isLoading by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }

    Column {
        OutlinedTextField(
            value = name,
            onValueChange = { name = it },
            label = { Text("Account name") },
            placeholder = { Text("My EteSync") },
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
        )

        Spacer(modifier = Modifier.height(16.dp))

        OutlinedTextField(
            value = serverUrl,
            onValueChange = { serverUrl = it },
            label = { Text("Server URL") },
            placeholder = { Text("https://api.etebase.com/") },
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
        )

        Spacer(modifier = Modifier.height(16.dp))

        OutlinedTextField(
            value = username,
            onValueChange = { username = it },
            label = { Text("Username / Email") },
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

        if (error != null) {
            Spacer(modifier = Modifier.height(16.dp))
            Text(
                text = error!!,
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall,
            )
        }

        Spacer(modifier = Modifier.height(24.dp))

        Button(
            onClick = {
                if (serverUrl.isBlank() || username.isBlank() || password.isBlank()) {
                    error = "Please fill in all fields"
                    return@Button
                }
                isLoading = true
                error = null
                scope.launch(Dispatchers.IO) {
                    try {
                        val client = DesktopEtebaseClient.login(
                            serverUrl = serverUrl.trim(),
                            username = username.trim(),
                            password = password,
                            caldavDao = application.caldavDao,
                        )
                        val session = client.getSession()
                        val account = CaldavAccount(
                            name = name.ifBlank { "EteSync" },
                            url = serverUrl.trim(),
                            username = username.trim(),
                            password = session,
                            accountType = CaldavAccount.TYPE_ETEBASE,
                        )
                        application.caldavDao.insert(account)
                        withContext(Dispatchers.Main) {
                            onAccountCreated()
                        }
                    } catch (e: Exception) {
                        withContext(Dispatchers.Main) {
                            error = e.message ?: "Failed to authenticate"
                            isLoading = false
                        }
                    }
                }
            },
            modifier = Modifier.fillMaxWidth(),
            enabled = !isLoading,
        ) {
            if (isLoading) {
                CircularProgressIndicator(
                    modifier = Modifier.height(20.dp).width(20.dp),
                    strokeWidth = 2.dp,
                )
            } else {
                Text("Sign In")
            }
        }
    }
}

private fun getAccountTypeTitle(type: AccountType): String {
    return when (type) {
        AccountType.CALDAV -> "CalDAV Account"
        AccountType.GOOGLE_TASKS -> "Google Tasks"
        AccountType.ETEBASE -> "EteSync Account"
    }
}
