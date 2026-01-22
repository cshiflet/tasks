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
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material.icons.filled.Sync
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.tasks.data.entity.CaldavAccount
import org.tasks.desktop.DesktopApplication
import org.tasks.desktop.platform.ThemeManager

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(
    application: DesktopApplication,
    onNavigateBack: () -> Unit,
    onAddAccount: () -> Unit,
    onEditAccount: (Long) -> Unit,
    modifier: Modifier = Modifier,
) {
    var accounts by remember { mutableStateOf<List<CaldavAccount>>(emptyList()) }
    var showThemeDialog by remember { mutableStateOf(false) }
    val themeManager = remember { application.container.themeManager }

    LaunchedEffect(Unit) {
        withContext(Dispatchers.IO) {
            accounts = application.caldavDao.getAccounts()
        }
    }

    if (showThemeDialog) {
        ThemePickerDialog(
            currentTheme = themeManager.themeMode,
            onThemeSelected = { mode ->
                themeManager.updateThemeMode(mode)
                showThemeDialog = false
            },
            onDismiss = { showThemeDialog = false }
        )
    }

    Scaffold(
        modifier = modifier.fillMaxSize(),
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        text = "Settings",
                        style = MaterialTheme.typography.titleLarge,
                    )
                },
                navigationIcon = {
                    IconButton(onClick = onNavigateBack) {
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
        ) {
            // Sync Accounts Section
            SectionHeader(title = "Sync Accounts")

            if (accounts.isEmpty()) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clickable(onClick = onAddAccount)
                        .padding(16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(
                        imageVector = Icons.Default.Add,
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.primary,
                    )
                    Spacer(modifier = Modifier.width(16.dp))
                    Text(
                        text = "Add sync account",
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.primary,
                    )
                }
            } else {
                accounts.forEach { account ->
                    AccountRow(
                        account = account,
                        onClick = { onEditAccount(account.id) }
                    )
                }
                HorizontalDivider()
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clickable(onClick = onAddAccount)
                        .padding(16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(
                        imageVector = Icons.Default.Add,
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.primary,
                    )
                    Spacer(modifier = Modifier.width(16.dp))
                    Text(
                        text = "Add another account",
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.primary,
                    )
                }
            }

            Spacer(modifier = Modifier.height(24.dp))

            // Appearance Section
            SectionHeader(title = "Appearance")
            SettingsRow(
                title = "Theme",
                subtitle = getThemeModeLabel(themeManager.themeMode),
                onClick = { showThemeDialog = true }
            )

            Spacer(modifier = Modifier.height(24.dp))

            // Task Defaults Section
            SectionHeader(title = "Task Defaults")
            SettingsRow(
                title = "Default list",
                subtitle = "My Tasks",
                onClick = { /* TODO: implement list picker */ }
            )
            SettingsRow(
                title = "Default priority",
                subtitle = "None",
                onClick = { /* TODO: implement priority picker */ }
            )

            Spacer(modifier = Modifier.height(24.dp))

            // About Section
            SectionHeader(title = "About")
            SettingsRow(
                title = "Version",
                subtitle = "Desktop Preview",
                onClick = { }
            )
            SettingsRow(
                title = "Source code",
                subtitle = "github.com/tasks/tasks",
                onClick = {
                    java.awt.Desktop.getDesktop().browse(java.net.URI("https://github.com/tasks/tasks"))
                }
            )
        }
    }
}

@Composable
private fun SectionHeader(title: String) {
    Text(
        text = title,
        style = MaterialTheme.typography.titleSmall,
        color = MaterialTheme.colorScheme.primary,
        modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp)
    )
}

@Composable
private fun AccountRow(
    account: CaldavAccount,
    onClick: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            imageVector = Icons.Default.Sync,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(modifier = Modifier.width(16.dp))
        Column(modifier = Modifier.weight(1f)) {
            Text(
                text = account.name ?: "Unknown",
                style = MaterialTheme.typography.bodyLarge,
            )
            if (account.error != null) {
                Text(
                    text = "Sync error",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                )
            } else {
                Text(
                    text = getAccountTypeLabel(account.accountType),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        Icon(
            imageVector = Icons.Default.ChevronRight,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun SettingsRow(
    title: String,
    subtitle: String,
    onClick: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(modifier = Modifier.weight(1f)) {
            Text(
                text = title,
                style = MaterialTheme.typography.bodyLarge,
            )
            Text(
                text = subtitle,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Icon(
            imageVector = Icons.Default.ChevronRight,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

private fun getAccountTypeLabel(accountType: Int): String {
    return when (accountType) {
        CaldavAccount.TYPE_CALDAV -> "CalDAV"
        CaldavAccount.TYPE_GOOGLE_TASKS -> "Google Tasks"
        CaldavAccount.TYPE_LOCAL -> "Local"
        else -> "Unknown"
    }
}

private fun getThemeModeLabel(mode: ThemeManager.ThemeMode): String {
    return when (mode) {
        ThemeManager.ThemeMode.LIGHT -> "Light"
        ThemeManager.ThemeMode.DARK -> "Dark"
        ThemeManager.ThemeMode.SYSTEM -> "System default"
    }
}

@Composable
private fun ThemePickerDialog(
    currentTheme: ThemeManager.ThemeMode,
    onThemeSelected: (ThemeManager.ThemeMode) -> Unit,
    onDismiss: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Choose theme") },
        text = {
            Column {
                ThemeManager.ThemeMode.entries.forEach { mode ->
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .selectable(
                                selected = currentTheme == mode,
                                onClick = { onThemeSelected(mode) },
                                role = Role.RadioButton
                            )
                            .padding(vertical = 12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        RadioButton(
                            selected = currentTheme == mode,
                            onClick = null,
                        )
                        Spacer(modifier = Modifier.width(12.dp))
                        Text(
                            text = getThemeModeLabel(mode),
                            style = MaterialTheme.typography.bodyLarge,
                        )
                    }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = onDismiss) {
                Text("Cancel")
            }
        },
    )
}
