package org.tasks.desktop.sync

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import org.tasks.caldav.VtodoCache
import org.tasks.data.dao.CaldavDao
import org.tasks.data.dao.DeletionDao
import org.tasks.data.dao.TaskDao
import org.tasks.data.entity.CaldavAccount
import java.util.concurrent.TimeUnit

class DesktopSyncManager(
    private val caldavDao: CaldavDao,
    private val taskDao: TaskDao,
    private val deletionDao: DeletionDao,
    private val vtodoCache: VtodoCache,
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var syncJob: Job? = null

    private val _isSyncing = MutableStateFlow(false)
    val isSyncing: StateFlow<Boolean> = _isSyncing.asStateFlow()

    private val _lastSyncTime = MutableStateFlow<Long?>(null)
    val lastSyncTime: StateFlow<Long?> = _lastSyncTime.asStateFlow()

    private val _syncError = MutableStateFlow<String?>(null)
    val syncError: StateFlow<String?> = _syncError.asStateFlow()

    // Auto-sync interval in milliseconds (15 minutes)
    private val syncInterval = TimeUnit.MINUTES.toMillis(15)

    fun startAutoSync() {
        syncJob?.cancel()
        syncJob = scope.launch {
            while (isActive) {
                syncAllAccounts()
                delay(syncInterval)
            }
        }
    }

    fun stopAutoSync() {
        syncJob?.cancel()
        syncJob = null
    }

    fun syncNow() {
        scope.launch {
            syncAllAccounts()
        }
    }

    suspend fun syncAccount(account: CaldavAccount) {
        _isSyncing.value = true
        _syncError.value = null
        try {
            when (account.accountType) {
                CaldavAccount.TYPE_CALDAV -> syncCaldavAccount(account)
                CaldavAccount.TYPE_GOOGLE_TASKS -> syncGoogleTasksAccount(account)
            }
            _lastSyncTime.value = System.currentTimeMillis()
            caldavDao.update(account.copy(error = null))
        } catch (e: Exception) {
            val errorMessage = e.message ?: "Unknown sync error"
            _syncError.value = errorMessage
            caldavDao.update(account.copy(error = errorMessage))
        } finally {
            _isSyncing.value = false
        }
    }

    private suspend fun syncAllAccounts() {
        val accounts = caldavDao.getAccounts()
        for (account in accounts) {
            syncAccount(account)
        }
    }

    private suspend fun syncCaldavAccount(account: CaldavAccount) {
        DesktopCaldavSynchronizer(
            caldavDao = caldavDao,
            taskDao = taskDao,
            deletionDao = deletionDao,
            vtodoCache = vtodoCache,
        ).sync(account)
    }

    private suspend fun syncGoogleTasksAccount(account: CaldavAccount) {
        // TODO: Implement Google Tasks sync (requires OAuth)
        // This will be implemented after OAuth is set up
    }

    companion object {
        @Volatile
        private var instance: DesktopSyncManager? = null

        fun getInstance(
            caldavDao: CaldavDao,
            taskDao: TaskDao,
            deletionDao: DeletionDao,
            vtodoCache: VtodoCache,
        ): DesktopSyncManager {
            return instance ?: synchronized(this) {
                instance ?: DesktopSyncManager(caldavDao, taskDao, deletionDao, vtodoCache).also {
                    instance = it
                }
            }
        }
    }
}
