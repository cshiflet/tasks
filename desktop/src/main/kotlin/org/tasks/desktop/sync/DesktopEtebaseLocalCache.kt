package org.tasks.desktop.sync

import com.etebase.client.Collection
import com.etebase.client.CollectionManager
import com.etebase.client.FileSystemCache
import com.etebase.client.Item
import com.etebase.client.ItemManager
import com.etebase.client.RemovedCollection
import com.etebase.client.exceptions.EtebaseException
import com.etebase.client.exceptions.UrlParseException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.tasks.desktop.platform.DesktopPaths

/**
 * JVM desktop equivalent of the Android [EtebaseLocalCache].
 *
 * Wraps the Etebase SDK's [FileSystemCache] with coroutine-friendly suspend functions.
 * Cache root: [DesktopPaths.etebaseCacheDir] / [username].
 *
 * One instance per username; obtain via [DesktopEtebaseLocalCache.getInstance].
 */
class DesktopEtebaseLocalCache private constructor(username: String) {
    private val fsCache: FileSystemCache =
        FileSystemCache.create(DesktopPaths.etebaseCacheDir.absolutePath, username)

    private suspend fun clearUserCache() = withContext(Dispatchers.IO) {
        fsCache.clearUserCache()
    }

    suspend fun saveStoken(stoken: String) = withContext(Dispatchers.IO) {
        fsCache.saveStoken(stoken)
    }

    suspend fun loadStoken(): String? = withContext(Dispatchers.IO) {
        fsCache.loadStoken()
    }

    suspend fun collectionList(colMgr: CollectionManager): List<Collection> =
        withContext(Dispatchers.IO) {
            fsCache._unstable_collectionList(colMgr).filter { !it.isDeleted }
        }

    suspend fun collectionGet(colMgr: CollectionManager, colUid: String): Collection =
        withContext(Dispatchers.IO) {
            fsCache.collectionGet(colMgr, colUid)
        }

    suspend fun collectionSet(colMgr: CollectionManager, collection: Collection) {
        if (collection.isDeleted) {
            collectionUnset(colMgr, collection.uid)
        } else {
            withContext(Dispatchers.IO) {
                fsCache.collectionSet(colMgr, collection)
            }
        }
    }

    suspend fun collectionUnset(colMgr: CollectionManager, removed: RemovedCollection) {
        collectionUnset(colMgr, removed.uid())
    }

    private suspend fun collectionUnset(colMgr: CollectionManager, colUid: String) {
        withContext(Dispatchers.IO) {
            try {
                fsCache.collectionUnset(colMgr, colUid)
            } catch (e: UrlParseException) {
                // File simply doesn't exist — safe to ignore
            }
        }
    }

    suspend fun itemGet(itemMgr: ItemManager, colUid: String, itemUid: String): Item? =
        withContext(Dispatchers.IO) {
            try {
                fsCache.itemGet(itemMgr, colUid, itemUid)
            } catch (e: EtebaseException) {
                null
            }
        }

    suspend fun itemSet(itemMgr: ItemManager, colUid: String, item: Item) {
        withContext(Dispatchers.IO) {
            if (item.isDeleted) {
                try {
                    fsCache.itemUnset(itemMgr, colUid, item.uid)
                } catch (e: UrlParseException) {
                    // File simply doesn't exist — safe to ignore
                }
            } else {
                fsCache.itemSet(itemMgr, colUid, item)
            }
        }
    }

    companion object {
        private val instances = HashMap<String, DesktopEtebaseLocalCache>()

        fun getInstance(username: String): DesktopEtebaseLocalCache =
            synchronized(instances) {
                instances.getOrPut(username) { DesktopEtebaseLocalCache(username) }
            }

        suspend fun clear(username: String) {
            val cache = synchronized(instances) { instances.remove(username) }
            cache?.clearUserCache()
        }
    }
}
