package org.tasks.desktop.sync

import co.touchlab.kermit.Logger
import com.etebase.client.Account
import com.etebase.client.Client
import com.etebase.client.Collection
import com.etebase.client.FetchOptions
import com.etebase.client.Item
import com.etebase.client.ItemMetadata
import com.etebase.client.exceptions.MsgPackException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import org.tasks.data.dao.CaldavDao
import org.tasks.data.entity.CaldavCalendar
import org.tasks.data.entity.CaldavTask
import org.tasks.time.DateTimeUtils2.currentTimeMillis
import java.util.concurrent.TimeUnit

private val LOG = Logger.withTag("DesktopEtebaseClient")

/**
 * JVM desktop equivalent of the Android [EtebaseClient].
 *
 * Wraps an authenticated Etebase [Account] and exposes suspend functions for
 * collection discovery, item fetching/uploading, and cache management.
 *
 * Obtain instances via [DesktopEtebaseClient.forAccount] (restores an existing session)
 * or [DesktopEtebaseClient.login] (authenticates with a username + password).
 */
class DesktopEtebaseClient(
    private val username: String,
    private val etebase: Account,
    private val caldavDao: CaldavDao,
) {
    private val cache = DesktopEtebaseLocalCache.getInstance(username)

    /** Serialise the current session so it can be persisted as the account password. */
    fun getSession(): String = etebase.save(null)

    // -----------------------------------------------------------------------
    // Collections
    // -----------------------------------------------------------------------

    suspend fun getCollections(): List<Collection> {
        val collectionManager = etebase.collectionManager
        var stoken: String? = cache.loadStoken()
        do {
            val response = withContext(Dispatchers.IO) {
                collectionManager.list(TYPE_TASKS, FetchOptions().stoken(stoken).limit(MAX_FETCH))
            }
            stoken = response.stoken
            response.data
                .filter { it.collectionType == TYPE_TASKS }
                .forEach { cache.collectionSet(collectionManager, it) }
            response.removedMemberships.forEach {
                cache.collectionUnset(collectionManager, it)
            }
        } while (!response.isDone)
        stoken?.let { cache.saveStoken(it) }
        return cache.collectionList(collectionManager)
    }

    suspend fun makeCollection(name: String, color: Int): String =
        etebase.collectionManager
            .create(TYPE_TASKS, ItemMetadata(), "")
            .let { setAndUpload(it, name, color) }

    suspend fun updateCollection(calendar: CaldavCalendar, name: String, color: Int): String =
        cache.collectionGet(etebase.collectionManager, calendar.url!!)
            .let { setAndUpload(it, name, color) }

    suspend fun deleteCollection(calendar: CaldavCalendar) =
        cache.collectionGet(etebase.collectionManager, calendar.url!!)
            .apply { delete() }
            .let { setAndUpload(it) }

    private suspend fun setAndUpload(
        collection: Collection,
        name: String? = null,
        color: Int? = null,
    ): String {
        collection.meta = collection.meta.let { meta ->
            name?.let { meta.name = it }
            color?.let { meta.color = it.toHexColor() }
            meta.mtime = currentTimeMillis()
            meta
        }
        val collectionManager = etebase.collectionManager
        withContext(Dispatchers.IO) { collectionManager.upload(collection) }
        cache.collectionSet(collectionManager, collection)
        return collection.uid
    }

    // -----------------------------------------------------------------------
    // Items
    // -----------------------------------------------------------------------

    suspend fun fetchItems(
        collection: Collection,
        calendar: CaldavCalendar,
        callback: suspend (Pair<String?, List<Item>>) -> Unit,
    ) {
        val itemManager = etebase.collectionManager.getItemManager(collection)
        var stoken = calendar.ctag
        do {
            val items = withContext(Dispatchers.IO) {
                itemManager.list(FetchOptions().stoken(stoken).limit(MAX_FETCH))
            }
            stoken = items.stoken
            callback(Pair(stoken, items.data.toList()))
        } while (!items.isDone)
    }

    suspend fun updateItem(collection: Collection, task: CaldavTask, content: ByteArray): Item {
        val itemManager = etebase.collectionManager.getItemManager(collection)
        val obj = task.obj
            ?: run {
                LOG.e { "null obj for caldavTask.id=${task.id}" }
                task.obj = task.remoteId
                task.obj
            }
            ?: throw IllegalStateException("Update failed — missing UUID")
        val item = cache.itemGet(itemManager, collection.uid, obj)
            ?: itemManager.create(ItemMetadata().apply { name = task.remoteId!! }, "")
                .apply {
                    task.obj = uid
                    caldavDao.update(task)
                }
        item.meta = updateMtime(item.meta, task.lastSync)
        item.content = content
        return item
    }

    suspend fun deleteItem(collection: Collection, task: CaldavTask): Item? {
        val itemManager = etebase.collectionManager.getItemManager(collection)
        val objId = task.obj
            ?: run {
                LOG.e { "null obj for caldavTask.id=${task.id}" }
                task.obj = task.remoteId
                task.obj
            }
            ?: return null
        return cache.itemGet(itemManager, collection.uid, objId)
            ?.takeIf { !it.isDeleted }
            ?.apply {
                meta = updateMtime(meta)
                delete()
            }
    }

    suspend fun updateCache(collection: Collection, items: List<Item>) {
        val itemManager = etebase.collectionManager.getItemManager(collection)
        items.forEach { cache.itemSet(itemManager, collection.uid, it) }
    }

    suspend fun uploadChanges(collection: Collection, items: List<Item>) {
        val itemManager = etebase.collectionManager.getItemManager(collection)
        withContext(Dispatchers.IO) { itemManager.batch(items.toTypedArray()) }
    }

    // -----------------------------------------------------------------------
    // Session management
    // -----------------------------------------------------------------------

    suspend fun logout() {
        try {
            DesktopEtebaseLocalCache.clear(username)
            withContext(Dispatchers.IO) { etebase.logout() }
        } catch (e: Exception) {
            LOG.e(e) { "logout failed" }
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    private fun updateMtime(
        meta: ItemMetadata,
        mtime: Long = currentTimeMillis(),
    ): ItemMetadata = meta.also { it.mtime = mtime }

    companion object {
        private const val TYPE_TASKS = "etebase.vtodo"
        private const val MAX_FETCH = 30L

        private fun Int.toHexColor(): String? = takeIf { this != 0 }
            ?.let { java.lang.String.format("#%06X", 0xFFFFFF and it) }

        private fun buildHttpClient(): OkHttpClient = OkHttpClient.Builder()
            .connectTimeout(15, TimeUnit.SECONDS)
            .readTimeout(120, TimeUnit.SECONDS)
            .writeTimeout(30, TimeUnit.SECONDS)
            .followRedirects(false)
            .followSslRedirects(false)
            .build()

        /**
         * Restore an existing Etebase session (normal sync path).
         * [session] is the string previously returned by [getSession].
         */
        suspend fun forAccount(
            serverUrl: String,
            username: String,
            session: String,
            caldavDao: CaldavDao,
        ): DesktopEtebaseClient = withContext(Dispatchers.IO) {
            val httpClient = buildHttpClient()
            val client = Client.create(httpClient, serverUrl)
            val etebase = try {
                Account.restore(client, session, null)
            } catch (e: MsgPackException) {
                throw IllegalStateException(
                    "EteSync session is invalid — please remove and re-add the account to authenticate.",
                    e,
                )
            }
            DesktopEtebaseClient(username, etebase, caldavDao)
        }

        /**
         * Authenticate with username + password and return a client whose session
         * can be persisted with [getSession].  Used only during account setup.
         */
        suspend fun login(
            serverUrl: String,
            username: String,
            password: String,
            caldavDao: CaldavDao,
        ): DesktopEtebaseClient = withContext(Dispatchers.IO) {
            val httpClient = buildHttpClient()
            val client = Client.create(httpClient, serverUrl)
            val etebase = Account.login(client, username, password)
            DesktopEtebaseClient(username, etebase, caldavDao)
        }
    }
}
