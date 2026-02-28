package org.tasks.desktop.sync

import co.touchlab.kermit.Logger
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.IOException
import java.net.ServerSocket
import java.net.URI
import java.security.MessageDigest
import java.security.SecureRandom
import java.util.Base64
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

private val LOG = Logger.withTag("DesktopGoogleTasksOAuth")

private const val AUTH_URL = "https://accounts.google.com/o/oauth2/v2/auth"
private const val TOKEN_URL = "https://oauth2.googleapis.com/token"
private const val USERINFO_URL = "https://www.googleapis.com/oauth2/v3/userinfo"
private const val TASKS_SCOPE = "https://www.googleapis.com/auth/tasks"
private const val USERINFO_SCOPE = "https://www.googleapis.com/auth/userinfo.email"

data class GoogleAuthResult(
    val email: String,
    val refreshToken: String,
)

/**
 * Performs an OAuth2 Authorization Code + PKCE flow for Google Tasks on the JVM desktop.
 *
 * Flow:
 *  1. Generate PKCE code verifier / challenge.
 *  2. Start a local [ServerSocket] on a random port to receive the redirect.
 *  3. Open the system browser to the Google consent screen.
 *  4. Receive the authorization code in the local server callback.
 *  5. Exchange the code for tokens using [clientId] + [clientSecret] + the code verifier.
 *  6. Fetch the user's email from the userinfo endpoint.
 *
 * @param clientId      Google OAuth2 client ID (Desktop app type).
 * @param clientSecret  Google OAuth2 client secret (required for token refresh).
 */
suspend fun authorizeGoogleTasks(
    clientId: String,
    clientSecret: String,
): GoogleAuthResult = withContext(Dispatchers.IO) {
    val codeVerifier = generateCodeVerifier()
    val codeChallenge = generateCodeChallenge(codeVerifier)

    // Bind to a random free port on loopback.
    val serverSocket = ServerSocket(0)
    val port = serverSocket.localPort
    val redirectUri = "http://127.0.0.1:$port"

    val latch = CountDownLatch(1)
    var authCode: String? = null
    var authError: String? = null

    // Background thread accepts exactly one HTTP request from the browser redirect.
    val serverThread = Thread {
        try {
            serverSocket.use { srv ->
                srv.soTimeout = 300_000 // 5 minutes
                srv.accept().use { client ->
                    val requestLine = client.getInputStream().bufferedReader().readLine() ?: ""
                    // requestLine is like: "GET /?code=AUTH_CODE&scope=... HTTP/1.1"
                    val queryString = requestLine
                        .removePrefix("GET /")
                        .substringBefore(" HTTP")
                        .substringAfter("?", "")
                    val params = queryString.split("&").associate {
                        val kv = it.split("=", limit = 2)
                        kv.getOrElse(0) { "" } to java.net.URLDecoder.decode(kv.getOrElse(1) { "" }, "UTF-8")
                    }
                    authCode = params["code"]
                    authError = params["error"]

                    val html = if (authCode != null) {
                        "<html><body><h2>Sign-in successful!</h2><p>You can close this tab and return to Tasks.</p></body></html>"
                    } else {
                        "<html><body><h2>Sign-in failed</h2><p>${authError ?: "Unknown error"}</p></body></html>"
                    }
                    val responseBytes = html.toByteArray(Charsets.UTF_8)
                    val response = buildString {
                        append("HTTP/1.1 200 OK\r\n")
                        append("Content-Type: text/html; charset=utf-8\r\n")
                        append("Content-Length: ${responseBytes.size}\r\n")
                        append("Connection: close\r\n")
                        append("\r\n")
                    }.toByteArray(Charsets.US_ASCII) + responseBytes
                    client.getOutputStream().write(response)
                }
            }
        } catch (e: IOException) {
            LOG.e(e) { "OAuth callback server error" }
        } finally {
            latch.countDown()
        }
    }
    serverThread.isDaemon = true
    serverThread.name = "google-oauth-callback"
    serverThread.start()

    val authorizationUrl = buildAuthorizationUrl(clientId, redirectUri, codeChallenge)
    try {
        java.awt.Desktop.getDesktop().browse(URI(authorizationUrl))
        LOG.i { "Opened browser to Google OAuth consent screen" }
    } catch (e: Exception) {
        LOG.e(e) { "Failed to open browser; manual URL: $authorizationUrl" }
    }

    val completed = latch.await(5, TimeUnit.MINUTES)
    if (!completed) {
        serverSocket.close()
        throw IllegalStateException("OAuth timed out: the browser authorization was not completed within 5 minutes.")
    }

    val code = authCode
        ?: throw IllegalStateException("OAuth failed: ${authError ?: "No authorization code received."}")

    val tokenResponse = exchangeCodeForTokens(
        clientId = clientId,
        clientSecret = clientSecret,
        code = code,
        redirectUri = redirectUri,
        codeVerifier = codeVerifier,
    )
    val email = fetchEmail(tokenResponse.accessToken)

    GoogleAuthResult(
        email = email,
        refreshToken = tokenResponse.refreshToken
            ?: throw IllegalStateException(
                "No refresh token in Google response. " +
                    "Ensure the OAuth client uses access_type=offline and prompt=consent."
            ),
    )
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

private data class RawTokenResponse(val accessToken: String, val refreshToken: String?)

private fun buildAuthorizationUrl(clientId: String, redirectUri: String, codeChallenge: String): String {
    val params = linkedMapOf(
        "client_id" to clientId,
        "redirect_uri" to redirectUri,
        "response_type" to "code",
        "scope" to "$TASKS_SCOPE $USERINFO_SCOPE",
        "code_challenge" to codeChallenge,
        "code_challenge_method" to "S256",
        "access_type" to "offline",
        "prompt" to "consent",
    )
    val query = params.entries.joinToString("&") { (k, v) ->
        "${java.net.URLEncoder.encode(k, "UTF-8")}=${java.net.URLEncoder.encode(v, "UTF-8")}"
    }
    return "$AUTH_URL?$query"
}

private fun exchangeCodeForTokens(
    clientId: String,
    clientSecret: String,
    code: String,
    redirectUri: String,
    codeVerifier: String,
): RawTokenResponse {
    val body = linkedMapOf(
        "grant_type" to "authorization_code",
        "code" to code,
        "redirect_uri" to redirectUri,
        "client_id" to clientId,
        "client_secret" to clientSecret,
        "code_verifier" to codeVerifier,
    ).entries.joinToString("&") { (k, v) ->
        "${java.net.URLEncoder.encode(k, "UTF-8")}=${java.net.URLEncoder.encode(v, "UTF-8")}"
    }

    val conn = java.net.URL(TOKEN_URL).openConnection() as java.net.HttpURLConnection
    conn.requestMethod = "POST"
    conn.setRequestProperty("Content-Type", "application/x-www-form-urlencoded")
    conn.doOutput = true
    conn.outputStream.use { it.write(body.toByteArray(Charsets.UTF_8)) }

    val statusCode = conn.responseCode
    val responseBody = if (statusCode == 200) {
        conn.inputStream.use { it.reader(Charsets.UTF_8).readText() }
    } else {
        val errorBody = conn.errorStream?.use { it.reader(Charsets.UTF_8).readText() } ?: ""
        throw IOException("Token exchange failed (HTTP $statusCode): $errorBody")
    }

    val accessToken = extractJsonString(responseBody, "access_token")
        ?: throw IOException("No access_token in Google token response")
    val refreshToken = extractJsonString(responseBody, "refresh_token")
    return RawTokenResponse(accessToken, refreshToken)
}

private fun fetchEmail(accessToken: String): String {
    val conn = java.net.URL(USERINFO_URL).openConnection() as java.net.HttpURLConnection
    conn.requestMethod = "GET"
    conn.setRequestProperty("Authorization", "Bearer $accessToken")
    val statusCode = conn.responseCode
    if (statusCode != 200) {
        throw IOException("Failed to fetch user info (HTTP $statusCode)")
    }
    val body = conn.inputStream.use { it.reader(Charsets.UTF_8).readText() }
    return extractJsonString(body, "email")
        ?: throw IOException("No 'email' field in Google userinfo response")
}

private fun generateCodeVerifier(): String {
    val bytes = ByteArray(32)
    SecureRandom().nextBytes(bytes)
    return Base64.getUrlEncoder().withoutPadding().encodeToString(bytes)
}

private fun generateCodeChallenge(verifier: String): String {
    val digest = MessageDigest.getInstance("SHA-256").digest(verifier.toByteArray(Charsets.US_ASCII))
    return Base64.getUrlEncoder().withoutPadding().encodeToString(digest)
}

/** Minimal JSON string-value extractor — avoids pulling in a JSON dependency just for two fields. */
private fun extractJsonString(json: String, key: String): String? =
    Regex("\"${Regex.escape(key)}\"\\s*:\\s*\"([^\"\\\\]*)\"").find(json)?.groupValues?.getOrNull(1)
