package org.tasks.desktop.sync

import co.touchlab.kermit.Logger
import com.etebase.client.Collection
import com.etebase.client.Item
import org.tasks.caldav.VtodoCache
import org.tasks.data.UUIDHelper
import org.tasks.data.dao.CaldavDao
import org.tasks.data.dao.DeletionDao
import org.tasks.data.dao.TaskDao
import org.tasks.data.entity.CaldavAccount
import org.tasks.data.entity.CaldavCalendar
import org.tasks.data.entity.CaldavTask
import org.tasks.data.entity.Task
import org.tasks.time.DateTimeUtils2.currentTimeMillis

private val LOG = Logger.withTag("DesktopEtesyncSynchronizer")

/**
 * JVM desktop equivalent of the Android [EtebaseSynchronizer].
 *
 * Performs a full bidirectional sync for a single [CaldavAccount] whose
 * [CaldavAccount.accountType] is [CaldavAccount.TYPE_ETEBASE].
 *
 * The account's [CaldavAccount.password] field stores the Etebase **session string**
 * (returned by [DesktopEtebaseClient.getSession] after initial login), not the
 * plaintext password.  On every sync, the session is restored from that string.
 *
 * Follows the same data-mapping conventions as the desktop CalDAV synchronizer:
 *  - VTODO parsing/serialization via [DesktopVtodoConverter]
 *  - Raw VTODO strings cached in [VtodoCache] for round-trip fidelity
 *  - Parent-task links updated via [CaldavDao.updateParents] after each calendar
 */
class DesktopEtesyncSynchronizer(
    private val caldavDao: CaldavDao,
    private val taskDao: TaskDao,
    private val deletionDao: DeletionDao,
    private val vtodoCache: VtodoCache,
) {
    // -----------------------------------------------------------------------
    // Entry point
    // -----------------------------------------------------------------------

    /**
     * Sync all Etebase collections for [account].
     *
     * @throws IllegalStateException when credentials are absent.
     */
    suspend fun sync(account: CaldavAccount) {
        val serverUrl = account.url?.takeIf { it.isNotBlank() }
            ?: throw IllegalStateException("EteSync account '${account.name}' has no server URL.")
        val username = account.username?.takeIf { it.isNotBlank() }
            ?: throw IllegalStateException("EteSync account '${account.name}' has no username.")
        val session = account.password?.takeIf { it.isNotBlank() }
            ?: throw IllegalStateException(
                "EteSync account '${account.name}' has no session. " +
                    "Please re-add the account to authenticate."
            )

        LOG.i { "Starting EteSync sync for '${account.name}' at $serverUrl" }

        val client = DesktopEtebaseClient.forAccount(serverUrl, username, session, caldavDao)
        synchronize(account, client)
    }

    // -----------------------------------------------------------------------
    // Core synchronization
    // -----------------------------------------------------------------------

    private suspend fun synchronize(account: CaldavAccount, client: DesktopEtebaseClient) {
        val collections = client.getCollections()
        val serverUids = collections.map { it.uid }
        LOG.d { "Found ${collections.size} remote collection(s): $serverUids" }

        // Remove local calendars that no longer exist on the server.
        for (deleted in caldavDao.findDeletedCalendars(account.uuid!!, serverUids)) {
            LOG.i { "Deleting local calendar '${deleted.name}' (no longer on server)" }
            deletionDao.delete(deleted) { _ ->
                vtodoCache.delete(deleted)
            }
        }

        for (collection in collections) {
            val uid = collection.uid
            val meta = collection.meta
            val color = meta.color
                ?.takeIf { it.isNotBlank() }
                ?.let { parseHexColor(it) }
                ?: 0

            var calendar = caldavDao.getCalendarByUrl(account.uuid!!, uid)
            if (calendar == null) {
                calendar = CaldavCalendar(
                    name = meta.name,
                    account = account.uuid,
                    url = uid,
                    uuid = UUIDHelper.newUUID(),
                    color = color,
                )
                caldavDao.insert(calendar)
                LOG.d { "Inserted new calendar '${calendar.name}'" }
            } else if (calendar.name != meta.name || calendar.color != color) {
                calendar.name = meta.name
                calendar.color = color
                caldavDao.update(calendar)
                LOG.d { "Updated calendar '${calendar.name}'" }
            }

            fetchChanges(account, client, calendar, collection)
            pushLocalChanges(account, client, calendar, collection)

            caldavDao.updateParents(calendar.uuid!!)
        }
    }

    // -----------------------------------------------------------------------
    // Fetch (download)
    // -----------------------------------------------------------------------

    private suspend fun fetchChanges(
        account: CaldavAccount,
        client: DesktopEtebaseClient,
        calendar: CaldavCalendar,
        collection: Collection,
    ) {
        if (calendar.ctag?.equals(collection.stoken) == true) {
            LOG.d { "'${calendar.name}' is up to date (stoken match)" }
            return
        }
        LOG.d { "Fetching changes for '${calendar.name}'" }
        client.fetchItems(collection, calendar) { (stoken, items) ->
            applyItems(account, calendar, items, stoken)
            client.updateCache(collection, items)
        }
        caldavDao.update(calendar)
    }

    private suspend fun applyItems(
        account: CaldavAccount,
        calendar: CaldavCalendar,
        items: List<Item>,
        stoken: String?,
        isLocalChange: Boolean = false,
    ) {
        for (item in items) {
            val vtodoString = item.contentString
            val parsed = DesktopVtodoConverter.parse(vtodoString)
            if (parsed == null) {
                LOG.e { "Failed to parse VTODO for item uid=${item.uid}" }
                continue
            }
            val remoteId = parsed.uid ?: item.uid

            // item.uid is the Etebase item UID, used as the CaldavTask.obj
            val existingCaldavTask = caldavDao.getTask(calendar.uuid!!, item.uid)

            if (item.isDeleted) {
                handleDeletedItem(calendar, existingCaldavTask)
                continue
            }

            if (existingCaldavTask?.isDeleted() == true) {
                // Locally deleted — our local deletion wins; skip incoming change.
                continue
            }

            if (isLocalChange) {
                existingCaldavTask?.let {
                    vtodoCache.putVtodo(calendar, it, vtodoString)
                    it.lastSync = item.meta.mtime ?: currentTimeMillis()
                    caldavDao.update(it)
                }
                continue
            }

            val task: Task = if (existingCaldavTask != null) {
                taskDao.fetch(existingCaldavTask.task) ?: createNewTask(calendar)
            } else {
                createNewTask(calendar)
            }

            val caldavTask: CaldavTask = existingCaldavTask
                ?: CaldavTask(
                    task = task.id,
                    calendar = calendar.uuid,
                    remoteId = remoteId,
                    obj = item.uid,
                )

            val locallyDirty = task.modificationDate > caldavTask.lastSync
                && caldavTask.lastSync != 0L

            if (!locallyDirty) {
                applyParsedToTask(parsed, task)
                task.suppressSync()
                task.suppressRefresh()
                taskDao.update(task)
            }

            val updatedCaldavTask = caldavTask.copy(
                task = task.id,
                remoteParent = parsed.parentUid,
                obj = item.uid,
                lastSync = item.meta.mtime ?: currentTimeMillis(),
            )
            vtodoCache.putVtodo(calendar, updatedCaldavTask, vtodoString)

            if (existingCaldavTask == null) {
                caldavDao.insert(updatedCaldavTask)
            } else {
                caldavDao.update(updatedCaldavTask)
            }
        }

        stoken?.let {
            calendar.ctag = it
            caldavDao.update(calendar)
        }
    }

    private suspend fun handleDeletedItem(
        calendar: CaldavCalendar,
        caldavTask: CaldavTask?,
    ) {
        if (caldavTask == null) return
        if (caldavTask.isDeleted()) {
            vtodoCache.delete(calendar, caldavTask)
            caldavDao.delete(caldavTask)
        } else {
            // Remote deletion of a non-locally-deleted task — cascade delete.
            val task = taskDao.fetch(caldavTask.task) ?: return
            taskDao.update(task.copy(deletionDate = currentTimeMillis()))
            vtodoCache.delete(calendar, caldavTask)
            caldavDao.delete(caldavTask)
        }
    }

    // -----------------------------------------------------------------------
    // Push (upload)
    // -----------------------------------------------------------------------

    private suspend fun pushLocalChanges(
        account: CaldavAccount,
        client: DesktopEtebaseClient,
        calendar: CaldavCalendar,
        collection: Collection,
    ) {
        val changes = ArrayList<Item>()

        // Tasks moved out of this calendar — delete their remote items.
        for (movedTask in caldavDao.getMoved(calendar.uuid!!)) {
            client.deleteItem(collection, movedTask)
                ?.let { changes.add(it) }
                ?: run {
                    vtodoCache.delete(calendar, movedTask)
                    caldavDao.delete(movedTask)
                }
        }

        // Locally modified tasks.
        for (taskWithCaldav in taskDao.getCaldavTasksToPush(calendar.uuid!!)) {
            val task = taskWithCaldav
            val caldavTask = caldavDao.getTask(task.id) ?: continue
            caldavTask.lastSync = task.modificationDate

            if (task.isDeleted) {
                client.deleteItem(collection, caldavTask)
                    ?.let { changes.add(it) }
                    ?: run {
                        taskDao.update(task)
                        vtodoCache.delete(calendar, caldavTask)
                        caldavDao.delete(caldavTask)
                    }
            } else {
                val existingVtodo = vtodoCache.getVtodo(calendar, caldavTask)
                val vtodoBytes = DesktopVtodoConverter.toVtodo(task, caldavTask, existingVtodo)
                changes.add(client.updateItem(collection, caldavTask, vtodoBytes))
            }
        }

        if (changes.isNotEmpty()) {
            client.uploadChanges(collection, changes)
            applyItems(account, calendar, changes, stoken = null, isLocalChange = true)
            client.updateCache(collection, changes)
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    private suspend fun createNewTask(calendar: CaldavCalendar): Task {
        val task = Task(
            creationDate = currentTimeMillis(),
            modificationDate = currentTimeMillis(),
            readOnly = calendar.readOnly(),
        )
        val id = taskDao.createNew(task)
        return task.copy(id = id)
    }

    private fun applyParsedToTask(parsed: DesktopVtodoConverter.ParsedVtodo, task: Task) {
        task.title = parsed.title?.takeIf { it.isNotBlank() } ?: task.title
        task.notes = parsed.notes
        task.priority = parsed.priority
        task.dueDate = parsed.dueDate
        task.hideUntil = parsed.hideUntil
        task.completionDate = parsed.completionDate
        task.recurrence = parsed.recurrence
        task.modificationDate =
            if (parsed.lastModified > 0) parsed.lastModified else currentTimeMillis()
    }

    /** Parse a CSS hex color string like "#RRGGBB" or "#RRGGBBAA" into an ARGB int. */
    private fun parseHexColor(hex: String): Int = try {
        val stripped = hex.trimStart('#')
        when (stripped.length) {
            6 -> {
                // No alpha — treat as fully opaque (0xFF_RRGGBB).
                val rgb = stripped.toInt(16)
                (0xFF shl 24) or rgb
            }
            8 -> {
                // Etebase stores alpha as the last two digits: #RRGGBBAA.
                // Convert to AARRGGBB (Android ARGB int).
                val r = stripped.substring(0, 2).toInt(16)
                val g = stripped.substring(2, 4).toInt(16)
                val b = stripped.substring(4, 6).toInt(16)
                val a = stripped.substring(6, 8).toInt(16)
                (a shl 24) or (r shl 16) or (g shl 8) or b
            }
            else -> 0
        }
    } catch (e: Exception) {
        0
    }
}
