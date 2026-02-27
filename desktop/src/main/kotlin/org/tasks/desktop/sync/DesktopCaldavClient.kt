package org.tasks.desktop.sync

import at.bitfire.dav4jvm.BasicDigestAuthHandler
import at.bitfire.dav4jvm.DavCollection
import at.bitfire.dav4jvm.DavResource
import at.bitfire.dav4jvm.Property
import at.bitfire.dav4jvm.Response
import at.bitfire.dav4jvm.Response.HrefRelation
import at.bitfire.dav4jvm.XmlUtils
import at.bitfire.dav4jvm.property.CalendarColor
import at.bitfire.dav4jvm.property.CalendarHomeSet
import at.bitfire.dav4jvm.property.CurrentUserPrincipal
import at.bitfire.dav4jvm.property.CurrentUserPrivilegeSet
import at.bitfire.dav4jvm.property.DisplayName
import at.bitfire.dav4jvm.property.GetCTag
import at.bitfire.dav4jvm.property.ResourceType
import at.bitfire.dav4jvm.property.ResourceType.Companion.CALENDAR
import at.bitfire.dav4jvm.property.SupportedCalendarComponentSet
import at.bitfire.dav4jvm.property.SyncToken
import co.touchlab.kermit.Logger
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.Interceptor
import okhttp3.OkHttpClient
import org.tasks.data.entity.CaldavCalendar.Companion.ACCESS_OWNER
import org.tasks.data.entity.CaldavCalendar.Companion.ACCESS_READ_ONLY
import org.tasks.data.entity.CaldavCalendar.Companion.ACCESS_READ_WRITE
import java.util.concurrent.TimeUnit

private val LOG = Logger.withTag("DesktopCaldavClient")

/**
 * CalDAV-specific HTTP client and server discovery for the desktop sync engine.
 *
 * Responsibilities:
 *  - Build an [OkHttpClient] with BasicDigest auth and appropriate timeouts.
 *  - Discover the calendar home set from a well-known URL or a user-provided URL.
 *  - List all VTODO-capable calendars under the home set.
 */
class DesktopCaldavClient(
    val httpClient: OkHttpClient,
    private val homeSetUrl: HttpUrl,
) {

    /**
     * Creates a new VTODO-capable calendar collection on the server under [homeSetUrl].
     * Sends an MKCOL request with the WebDAV Extended MKCOL XML body.
     *
     * @return the canonical URL of the newly created calendar.
     */
    suspend fun makeCollection(displayName: String, color: Int): String = withContext(Dispatchers.IO) {
        val uuid = java.util.UUID.randomUUID().toString()
        val calendarUrl = homeSetUrl.resolve("$uuid/")
            ?: throw IllegalStateException("Cannot resolve new calendar URL under $homeSetUrl")
        val xmlBody = buildMkcolXml(displayName, color)
        val resource = DavResource(httpClient, calendarUrl)
        resource.mkCol(xmlBody) {}
        resource.location.toString()
    }

    /**
     * Updates the display name and (optionally) color of an existing remote calendar via PROPPATCH.
     */
    suspend fun updateCollection(calendarUrl: String, displayName: String, color: Int) =
        withContext(Dispatchers.IO) {
            val url = calendarUrl.toHttpUrl()
            val setProps = buildMap<Property.Name, String> {
                put(DisplayName.NAME, displayName)
                if (color != 0) {
                    put(
                        CalendarColor.NAME,
                        String.format(
                            "#%06X%02X",
                            color and 0xFFFFFF,
                            (color.toLong() ushr 24 and 0xFF).toInt(),
                        ),
                    )
                }
            }
            DavResource(httpClient, url).proppatch(
                setProperties = setProps,
                removeProperties = emptyList(),
                callback = { _, _ -> },
            )
        }

    /**
     * Returns all calendars under [homeSetUrl] that support VTODOs.
     * Each [Response] carries [DisplayName], [GetCTag]/[SyncToken], [CalendarColor], and
     * [CurrentUserPrivilegeSet] so that the synchronizer can decide on access level.
     */
    suspend fun calendars(): List<Response> = withContext(Dispatchers.IO) {
        val results = ArrayList<Pair<Response, HrefRelation>>()
        DavCollection(httpClient, homeSetUrl).propfind(
            depth = 1,
            *CALENDAR_PROPERTIES,
        ) { response, relation ->
            results.add(response to relation)
        }
        results
            .filter { (response, relation) ->
                relation == HrefRelation.MEMBER &&
                    response[ResourceType::class.java]?.types?.contains(CALENDAR) == true &&
                    response[SupportedCalendarComponentSet::class.java]?.supportsTasks == true
            }
            .map { (response, _) -> response }
    }

    companion object {
        /** Calendar properties to request in PROPFIND depth-1. */
        private val CALENDAR_PROPERTIES = arrayOf(
            ResourceType.NAME,
            DisplayName.NAME,
            SupportedCalendarComponentSet.NAME,
            GetCTag.NAME,
            SyncToken.NAME,
            CalendarColor.NAME,
            CurrentUserPrivilegeSet.NAME,
            CurrentUserPrincipal.NAME,
        )

        /**
         * Derive an integer access level from a calendar PROPFIND [Response].
         * Falls back to read-write if the server did not advertise privilege information.
         */
        val Response.accessLevel: Int
            get() = when (this[CurrentUserPrivilegeSet::class.java]?.mayWriteContent) {
                false -> ACCESS_READ_ONLY
                else -> ACCESS_READ_WRITE
            }

        /** Combined ctag/sync-token used for change detection. */
        val Response.ctag: String?
            get() = this[SyncToken::class.java]?.token ?: this[GetCTag::class.java]?.cTag

        /**
         * Build an [OkHttpClient] wired with BasicDigest authentication and standard CalDAV
         * timeouts.  Logs at DEBUG level so sync issues can be diagnosed from logs.
         */
        fun buildHttpClient(username: String, password: String): OkHttpClient {
            val authHandler = BasicDigestAuthHandler(
                domain = null,
                username = username,
                password = password,
            )
            return OkHttpClient.Builder()
                .connectTimeout(15, TimeUnit.SECONDS)
                .readTimeout(120, TimeUnit.SECONDS)
                .writeTimeout(30, TimeUnit.SECONDS)
                // dav4jvm requires manual redirect handling to track URL changes
                .followRedirects(false)
                .followSslRedirects(false)
                .addNetworkInterceptor(authHandler)
                .authenticator(authHandler)
                .addInterceptor(Interceptor { chain ->
                    val request = chain.request()
                    Logger.d("DesktopCaldavHttp") {
                        "${request.method} ${request.url}"
                    }
                    val response = chain.proceed(request)
                    Logger.d("DesktopCaldavHttp") {
                        "${response.code} ${request.url}"
                    }
                    response
                })
                .build()
        }

        /**
         * Discover the CalDAV home set URL for [account].
         *
         * Strategy (mirrors the Android [CaldavClient.homeSet]):
         *  1. PROPFIND /.well-known/caldav for [CurrentUserPrincipal].
         *  2. If that fails or returns nothing, PROPFIND the raw [serverUrl] itself.
         *  3. PROPFIND the principal URL for [CalendarHomeSet].
         *
         * @throws IllegalStateException if no home set can be found.
         */
        suspend fun discoverHomeSet(
            httpClient: OkHttpClient,
            serverUrl: String,
        ): HttpUrl = withContext(Dispatchers.IO) {
            val base = serverUrl.toHttpUrl()

            // Step 1 – try well-known endpoint.
            var principalUrl: String? = null
            try {
                principalUrl = tryFindPrincipal(httpClient, base, "/.well-known/caldav")
            } catch (e: Exception) {
                LOG.w(e) { "Well-known CalDAV lookup failed: ${e.message}" }
            }

            // Step 2 – fall back to the base URL itself.
            if (principalUrl == null) {
                try {
                    principalUrl = tryFindPrincipal(httpClient, base, "")
                } catch (e: Exception) {
                    LOG.w(e) { "Base URL principal lookup failed: ${e.message}" }
                }
            }

            val principalHttpUrl = if (principalUrl.isNullOrBlank()) {
                base
            } else {
                base.resolve(principalUrl) ?: base
            }

            // Step 3 – PROPFIND the principal for CalendarHomeSet.
            val homeSetResponses = ArrayList<Pair<Response, HrefRelation>>()
            DavResource(httpClient, principalHttpUrl).propfind(0, CalendarHomeSet.NAME) { r, rel ->
                homeSetResponses.add(r to rel)
            }
            val homeSetHref = homeSetResponses
                .firstOrNull()
                ?.first
                ?.get(CalendarHomeSet::class.java)
                ?.href
                ?.takeIf { it.isNotBlank() }
                ?: throw IllegalStateException(
                    "CalDAV home set not found for $serverUrl. " +
                        "Verify the server URL and credentials."
                )

            principalHttpUrl.resolve(homeSetHref)
                ?: throw IllegalStateException(
                    "Cannot resolve home set href '$homeSetHref' against '$principalHttpUrl'"
                )
        }

        /**
         * PROPFIND [path] on [base] and return the [CurrentUserPrincipal] href if present.
         */
        private suspend fun tryFindPrincipal(
            httpClient: OkHttpClient,
            base: HttpUrl,
            path: String,
        ): String? {
            val url = base.resolve(path) ?: return null
            val responses = ArrayList<Pair<Response, HrefRelation>>()
            DavResource(httpClient, url).propfind(0, CurrentUserPrincipal.NAME) { r, rel ->
                responses.add(r to rel)
            }
            return responses
                .firstOrNull()
                ?.first
                ?.get(CurrentUserPrincipal::class.java)
                ?.href
                ?.takeIf { it.isNotBlank() }
        }

        /**
         * Builds the Extended MKCOL XML body for creating a VTODO-capable calendar collection.
         * Mirrors the Android CaldavClient.getMkcolString().
         */
        private fun buildMkcolXml(displayName: String, color: Int): String {
            val colorXml = if (color != 0) {
                val hex = String.format(
                    "#%06X%02X",
                    color and 0xFFFFFF,
                    (color.toLong() ushr 24 and 0xFF).toInt(),
                )
                "<IC:calendar-color xmlns:IC=\"${XmlUtils.NS_APPLE_ICAL}\">$hex</IC:calendar-color>"
            } else ""
            val safeName = displayName
                .replace("&", "&amp;")
                .replace("<", "&lt;")
                .replace(">", "&gt;")
            return """<?xml version="1.0" encoding="UTF-8"?>
<mkcol xmlns="${XmlUtils.NS_WEBDAV}" xmlns:CAL="${XmlUtils.NS_CALDAV}">
  <set>
    <prop>
      <resourcetype>
        <collection/>
        <CAL:calendar/>
      </resourcetype>
      <displayname>$safeName</displayname>
      $colorXml
      <CAL:supported-calendar-component-set>
        <CAL:comp name="VTODO"/>
      </CAL:supported-calendar-component-set>
    </prop>
  </set>
</mkcol>"""
        }

        /**
         * Convenience: build a fully initialised [DesktopCaldavClient] for [serverUrl].
         */
        suspend fun forAccount(
            serverUrl: String,
            username: String,
            password: String,
        ): DesktopCaldavClient {
            val httpClient = buildHttpClient(username, password)
            val homeSetUrl = discoverHomeSet(httpClient, serverUrl)
            return DesktopCaldavClient(httpClient, homeSetUrl)
        }
    }
}
