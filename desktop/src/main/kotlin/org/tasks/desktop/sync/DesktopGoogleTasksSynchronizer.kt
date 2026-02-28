package org.tasks.desktop.sync

import co.touchlab.kermit.Logger
import com.google.api.client.googleapis.json.GoogleJsonResponseException
import com.google.api.client.util.DateTime
import com.google.api.services.tasks.model.Task
import com.google.api.services.tasks.model.TaskList
import kotlinx.coroutines.delay
import org.tasks.data.dao.CaldavDao
import org.tasks.data.dao.DeletionDao
import org.tasks.data.dao.GoogleTaskDao
import org.tasks.data.dao.TaskDao
import org.tasks.data.entity.CaldavAccount
import org.tasks.data.entity.CaldavCalendar
import org.tasks.data.entity.CaldavTask
import org.tasks.time.DateTimeUtils2.currentTimeMillis
import java.io.IOException

private val LOG = Logger.withTag("DesktopGoogleTasksSynchronizer")

/**
 * Desktop equivalent of the Android [GoogleTaskSynchronizer].
 *
 * Credentials model (stored on [CaldavAccount]):
 *  - [CaldavAccount.url]      = `"${clientId}:::${clientSecret}"`
 *  - [CaldavAccount.username] = Google account email
 *  - [CaldavAccount.password] = OAuth2 refresh token
 *
 * Sync logic follows the same sequence as the Android version:
 *  1. Fetch remote task lists and update local [CaldavCalendar] rows.
 *  2. Push local changes (new, modified, deleted) to Google Tasks.
 *  3. Fetch remote changes and apply them to local tasks.
 *  4. Update task positions (parent / order) from the remote ordering.
 */
class DesktopGoogleTasksSynchronizer(
    private val caldavDao: CaldavDao,
    private val taskDao: TaskDao,
    private val googleTaskDao: GoogleTaskDao,
    private val deletionDao: DeletionDao,
) {
    @Throws(IOException::class)
    suspend fun sync(account: CaldavAccount) {
        val (clientId, clientSecret) = parseClientCredentials(account)
        val refreshToken = account.password
            ?.takeIf { it.isNotBlank() }
            ?: throw IllegalStateException("Google Tasks account '${account.name}' has no refresh token.")

        LOG.i { "Starting Google Tasks sync for '${account.name}'" }

        val invoker = DesktopGoogleTasksInvoker(clientId, clientSecret, refreshToken)
        synchronize(account, invoker)
    }

    // -----------------------------------------------------------------------
    // Core synchronization
    // -----------------------------------------------------------------------

    @Throws(IOException::class)
    private suspend fun synchronize(account: CaldavAccount, invoker: DesktopGoogleTasksInvoker) {
        // 1. Fetch all remote task lists.
        val remoteLists = mutableListOf<TaskList>()
        var nextPageToken: String? = null
        do {
            val page = invoker.allGtaskLists(nextPageToken) ?: break
            page.items?.let { remoteLists.addAll(it) }
            nextPageToken = page.nextPageToken
        } while (!nextPageToken.isNullOrEmpty())

        // 2. Reconcile local calendars with the remote list.
        updateLists(account, remoteLists)

        // 3. Push local changes, retrying when a stale task ID is encountered.
        val failedTasks = mutableSetOf<Long>()
        var retryTaskId = pushLocalChanges(account, invoker)
        while (retryTaskId != null) {
            if (failedTasks.contains(retryTaskId)) {
                throw IOException("Invalid Google Task ID for local task $retryTaskId — aborting.")
            }
            failedTasks.add(retryTaskId)
            LOG.d { "Retrying push after stale task ID $retryTaskId" }
            delay(1_000)
            retryTaskId = pushLocalChanges(account, invoker)
        }

        // 4. Fetch and apply remote changes, then update positions.
        for (list in caldavDao.getCalendarsByAccount(account.uuid!!)) {
            if (list.uuid.isNullOrEmpty()) continue
            fetchAndApplyRemoteChanges(invoker, list)
            updatePositions(invoker, list.uuid!!)
        }
    }

    // -----------------------------------------------------------------------
    // List management
    // -----------------------------------------------------------------------

    private suspend fun updateLists(account: CaldavAccount, remoteLists: List<TaskList>) {
        val localLists = caldavDao.getCalendarsByAccount(account.uuid!!)
        val localByUuid = localLists.associateBy { it.uuid }
        val localIds = localLists.map { it.id }.toMutableSet()

        for (remote in remoteLists) {
            val local = localByUuid[remote.id] ?: CaldavCalendar(
                account = account.uuid,
                uuid = remote.id,
            )
            caldavDao.insertOrReplace(local.copy(name = remote.title))
            localIds.remove(local.id)
        }

        // Delete local lists no longer present on the remote.
        for (orphanId in localIds) {
            val orphan = caldavDao.getCalendarById(orphanId) ?: continue
            LOG.i { "Deleting local list '${orphan.name}' (removed from Google Tasks)" }
            deletionDao.delete(orphan) { _ -> /* no local file cleanup needed */ }
        }
    }

    // -----------------------------------------------------------------------
    // Push (local → remote)
    // -----------------------------------------------------------------------

    @Throws(IOException::class)
    private suspend fun pushLocalChanges(
        account: CaldavAccount,
        invoker: DesktopGoogleTasksInvoker,
    ): Long? {
        val tasks = taskDao.getGoogleTasksToPush(account.uuid!!)
        for (task in tasks) {
            val retryId = pushTask(task, account.uuid!!, invoker)
            if (retryId != null) return retryId
        }
        return null
    }

    @Throws(IOException::class)
    private suspend fun pushTask(
        task: org.tasks.data.entity.Task,
        account: String,
        invoker: DesktopGoogleTasksInvoker,
    ): Long? {
        // Delete any pending remote deletions for this task.
        for (deleted in googleTaskDao.getDeletedByTaskId(task.id, account)) {
            deleted.remoteId?.let { remoteId ->
                try {
                    invoker.deleteGtask(deleted.calendar, remoteId)
                } catch (e: GoogleJsonResponseException) {
                    if (e.statusCode != 400) throw e
                    LOG.w { "HTTP 400 deleting task $remoteId — ignored" }
                }
            }
            googleTaskDao.delete(deleted)
        }

        val gtasksMetadata = googleTaskDao.getByTaskId(task.id) ?: return null
        val remoteModel = Task()
        val newlyCreated: Boolean

        var listId = gtasksMetadata.calendar ?: DEFAULT_LIST

        if (gtasksMetadata.remoteId.isNullOrEmpty()) {
            // Create case.
            newlyCreated = true
            if (!gtasksMetadata.calendar.isNullOrEmpty()) listId = gtasksMetadata.calendar!!
        } else {
            // Update case.
            newlyCreated = false
            remoteModel.id = gtasksMetadata.remoteId
            listId = gtasksMetadata.calendar!!
        }

        // Skip tasks that are newly created with no title, or newly created + already deleted.
        if (newlyCreated && (task.title.isNullOrEmpty() || task.deletionDate > 0)) return null

        // Populate remote model.
        if (task.isDeleted) remoteModel.deleted = true
        remoteModel.title = truncate(task.title, MAX_TITLE_LENGTH)
        remoteModel.notes = truncate(task.notes, MAX_DESCRIPTION_LENGTH)
        if (task.hasDueDate()) {
            remoteModel.due = invoker.unixTimeToGtasksDueDate(task.dueDate)?.toStringRfc3339()
        }
        if (task.isCompleted) {
            remoteModel.completed = invoker.unixTimeToGtasksCompletionTime(task.completionDate).toStringRfc3339()
            remoteModel.status = "completed"
        } else {
            remoteModel.completed = null
            remoteModel.status = "needsAction"
        }

        if (newlyCreated) {
            val parent = task.parent
            val localParent = if (parent > 0) googleTaskDao.getRemoteId(parent, listId) else null
            val previous = googleTaskDao.getPrevious(
                listId,
                if (localParent.isNullOrEmpty()) 0L else parent,
                task.order ?: 0L,
            )
            val created: Task? = try {
                invoker.createGtask(listId, remoteModel, localParent, previous)
            } catch (e: GoogleJsonResponseException) {
                if (e.statusCode == 404) {
                    LOG.w { "HTTP 404 creating task — retrying without parent/order" }
                    invoker.createGtask(listId, remoteModel, null, null)
                } else throw e
            }
            if (created != null) {
                gtasksMetadata.remoteId = created.id
                gtasksMetadata.calendar = listId
                setOrderAndParent(gtasksMetadata, created, task)
            } else {
                LOG.e { "Empty response creating task — skipping" }
                return null
            }
        } else {
            try {
                if (!task.isDeleted && gtasksMetadata.isMoved) {
                    try {
                        val parent = task.parent
                        val localParent = if (parent > 0) googleTaskDao.getRemoteId(parent, listId) else null
                        val previous = googleTaskDao.getPrevious(
                            listId,
                            if (localParent.isNullOrEmpty()) 0L else parent,
                            task.order ?: 0L,
                        )
                        invoker.moveGtask(
                            listId = listId,
                            taskId = remoteModel.id,
                            parentId = localParent,
                            previousId = previous,
                        )?.let { setOrderAndParent(gtasksMetadata, it, task) }
                    } catch (e: GoogleJsonResponseException) {
                        if (e.statusCode == 400) {
                            LOG.w { "HTTP 400 moving task — clearing parent/order" }
                            taskDao.setParent(0L, listOf(task.id))
                            taskDao.setOrder(task.id, 0L)
                            googleTaskDao.update(gtasksMetadata.copy(isMoved = false))
                            return task.id
                        } else throw e
                    }
                }
                try {
                    invoker.updateGtask(listId, remoteModel)
                } catch (e: GoogleJsonResponseException) {
                    if (e.statusCode == 400 && e.details?.message == "Invalid task ID") {
                        LOG.w { "HTTP 400 Invalid task ID ${remoteModel.id} — will recreate on next sync" }
                        googleTaskDao.update(gtasksMetadata.copy(remoteId = "", isMoved = false))
                        return task.id
                    } else throw e
                }
            } catch (e: GoogleJsonResponseException) {
                if (e.statusCode == 404) {
                    LOG.w { "HTTP 404 — deleting stale gtasks metadata" }
                    googleTaskDao.delete(gtasksMetadata)
                    return null
                } else throw e
            }
        }

        gtasksMetadata.isMoved = false
        write(task, gtasksMetadata)
        return null
    }

    // -----------------------------------------------------------------------
    // Fetch (remote → local)
    // -----------------------------------------------------------------------

    @Throws(IOException::class)
    private suspend fun fetchAndApplyRemoteChanges(
        invoker: DesktopGoogleTasksInvoker,
        list: CaldavCalendar,
    ) {
        val listId = list.uuid ?: return
        var lastSyncDate = list.lastSync
        val tasks = mutableListOf<Task>()
        var nextPageToken: String? = null
        do {
            val page = try {
                invoker.getAllGtasksFromListId(listId, lastSyncDate + 1_000L, nextPageToken)
            } catch (e: GoogleJsonResponseException) {
                if (e.statusCode == 404) {
                    LOG.w { "HTTP 404 fetching list $listId — skipping" }
                    return
                } else throw e
            } ?: break
            page.items?.let { tasks.addAll(it) }
            nextPageToken = page.nextPageToken
        } while (!nextPageToken.isNullOrEmpty())

        // Process parents before children to satisfy foreign key order.
        tasks.sortWith(PARENTS_FIRST)

        for (gtask in tasks) {
            val remoteId = gtask.id ?: continue
            var googleTask = googleTaskDao.getByRemoteId(remoteId, listId)
            var task: org.tasks.data.entity.Task? = null

            if (googleTask == null) {
                googleTask = CaldavTask(task = 0, calendar = listId, remoteId = null)
            } else if (googleTask.task > 0) {
                task = taskDao.fetch(googleTask.task)
            }

            gtask.updated?.let {
                val updatedMs = DateTime(it).value
                if (updatedMs > lastSyncDate) lastSyncDate = updatedMs
            }

            val isDeleted = gtask.deleted == true
            val isHidden = gtask.hidden == true

            if (isDeleted) {
                task?.let { taskDao.update(it.copy(deletionDate = currentTimeMillis())) }
                continue
            } else if (isHidden) {
                if (task == null) continue
                if (task.isRecurring) {
                    googleTask.remoteId = ""
                } else {
                    taskDao.update(task.copy(deletionDate = currentTimeMillis()))
                    continue
                }
            } else {
                if (task == null) {
                    val newTask = org.tasks.data.entity.Task(
                        creationDate = currentTimeMillis(),
                        modificationDate = currentTimeMillis(),
                    )
                    val newId = taskDao.createNew(newTask)
                    task = newTask.copy(id = newId)
                }
                setOrderAndParent(googleTask, gtask, task)
                googleTask.remoteId = gtask.id
            }

            task.title = getTruncatedValue(task.title, gtask.title, MAX_TITLE_LENGTH)
            task.completionDate = invoker.gtasksCompletedTimeToUnixTime(gtask.completed?.let(::DateTime))
            val dueDate = invoker.gtasksDueTimeToUnixTime(gtask.due?.let(::DateTime))
            mergeDates(createDueDateDayOnly(dueDate), task)
            task.notes = getTruncatedValue(task.notes, gtask.notes, MAX_DESCRIPTION_LENGTH)

            if (!task.title.isNullOrBlank() || !task.notes.isNullOrBlank()) {
                write(task, googleTask)
            }
        }

        caldavDao.insertOrReplace(list.copy(lastSync = lastSyncDate))
    }

    @Throws(IOException::class)
    private suspend fun updatePositions(invoker: DesktopGoogleTasksInvoker, listId: String) {
        val tasks = mutableListOf<Task>()
        var nextPageToken: String? = null
        do {
            val page = invoker.getAllPositions(listId, nextPageToken) ?: break
            page.items?.let { tasks.addAll(it) }
            nextPageToken = page.nextPageToken
        } while (!nextPageToken.isNullOrEmpty())

        for (task in tasks) {
            googleTaskDao.updatePosition(task.id, task.parent, task.position)
        }
        googleTaskDao.reposition(caldavDao, listId)
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    private suspend fun setOrderAndParent(
        googleTask: CaldavTask,
        remote: Task,
        local: org.tasks.data.entity.Task,
    ) {
        remote.position?.toLongOrNull()?.let { googleTask.remoteOrder = it }
        googleTask.remoteParent = remote.parent?.takeIf { it.isNotBlank() }
        local.parent = googleTask.remoteParent
            ?.let { googleTaskDao.getTask(it, googleTask.calendar!!) }
            ?: 0L
    }

    private suspend fun write(
        task: org.tasks.data.entity.Task,
        googleTask: CaldavTask,
    ) {
        task.suppressSync()
        task.suppressRefresh()
        if (task.isNew) taskDao.createNew(task)
        taskDao.update(task)
        val updated = googleTask.copy(task = task.id, lastSync = task.modificationDate)
        if (updated.id == 0L) googleTaskDao.insert(updated) else googleTaskDao.update(updated)
    }

    companion object {
        private const val DEFAULT_LIST = "@default"
        private const val MAX_TITLE_LENGTH = 1024
        private const val MAX_DESCRIPTION_LENGTH = 8192

        private val PARENTS_FIRST = Comparator { o1: Task, o2: Task ->
            val p1 = o1.parent.isNullOrEmpty()
            val p2 = o2.parent.isNullOrEmpty()
            when {
                p1 && p2 -> 0
                p1 -> -1
                p2 -> 1
                else -> 0
            }
        }

        fun truncate(string: String?, max: Int): String? =
            if (string == null || string.length <= max) string else string.substring(0, max)

        fun getTruncatedValue(current: String?, incoming: String?, maxLength: Int): String? =
            if (incoming.isNullOrEmpty() || incoming.length < maxLength || current.isNullOrEmpty()
                || !current.startsWith(incoming)
            ) incoming else current

        fun mergeDates(remoteDueDate: Long, local: org.tasks.data.entity.Task) {
            if (remoteDueDate > 0 && local.hasDueTime()) {
                // Graft the local time-of-day onto the new remote date.
                val localMs = local.dueDate
                val oldHour = ((localMs / 3_600_000L) % 24).toInt()
                val oldMin = ((localMs / 60_000L) % 60).toInt()
                val oldSec = ((localMs / 1_000L) % 60).toInt()
                val adjusted = remoteDueDate + (oldHour * 3_600_000L) + (oldMin * 60_000L) + (oldSec * 1_000L)
                local.setDueDateAdjustingHideUntil(createDueDateWithTime(adjusted))
            } else {
                local.setDueDateAdjustingHideUntil(remoteDueDate)
            }
        }

        /**
         * Equivalent of createDueDate(URGENCY_SPECIFIC_DAY, epochMillis).
         * Sets time to 12:00:00 (seconds == 0 signals no due time).
         */
        fun createDueDateDayOnly(epochMillis: Long): Long {
            if (epochMillis <= 0) return 0L
            val cal = java.util.Calendar.getInstance()
            cal.timeInMillis = epochMillis
            cal.set(java.util.Calendar.HOUR_OF_DAY, 12)
            cal.set(java.util.Calendar.MINUTE, 0)
            cal.set(java.util.Calendar.SECOND, 0)
            cal.set(java.util.Calendar.MILLISECOND, 0)
            return cal.timeInMillis
        }

        /**
         * Equivalent of createDueDate(URGENCY_SPECIFIC_DAY_TIME, epochMillis).
         * Sets seconds to 1 to signal "due time exists".
         */
        fun createDueDateWithTime(epochMillis: Long): Long {
            if (epochMillis <= 0) return 0L
            val cal = java.util.Calendar.getInstance()
            cal.timeInMillis = epochMillis
            cal.set(java.util.Calendar.SECOND, 1)
            cal.set(java.util.Calendar.MILLISECOND, 0)
            return cal.timeInMillis
        }

        /**
         * Parse `clientId:::clientSecret` stored in [CaldavAccount.url].
         * Returns a pair of (clientId, clientSecret).
         */
        fun parseClientCredentials(account: CaldavAccount): Pair<String, String> {
            val raw = account.url?.takeIf { it.isNotBlank() }
                ?: throw IllegalStateException(
                    "Google Tasks account '${account.name}' has no client credentials (url is blank)."
                )
            val parts = raw.split(":::")
            if (parts.size < 2) {
                throw IllegalStateException(
                    "Google Tasks account '${account.name}' has malformed credentials in the url field. " +
                        "Expected 'clientId:::clientSecret'."
                )
            }
            return parts[0] to parts[1]
        }
    }
}
