package org.tasks.desktop.auth

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import okhttp3.FormBody
import okhttp3.OkHttpClient
import okhttp3.Request
import java.awt.Desktop
import java.net.URI
import java.net.URLEncoder

class DesktopOAuthHandler(
    private val httpClient: OkHttpClient = OkHttpClient()
) {
    private val callbackServer = OAuthCallbackServer()
    private val json = Json { ignoreUnknownKeys = true }

    data class TokenResponse(
        val accessToken: String,
        val refreshToken: String?,
        val expiresIn: Long,
        val tokenType: String,
    )

    data class OAuthConfig(
        val clientId: String,
        val clientSecret: String,
        val authorizationEndpoint: String,
        val tokenEndpoint: String,
        val scopes: List<String>,
    )

    companion object {
        val GOOGLE_TASKS_CONFIG = OAuthConfig(
            clientId = "", // User needs to provide their own client ID
            clientSecret = "", // User needs to provide their own client secret
            authorizationEndpoint = "https://accounts.google.com/o/oauth2/v2/auth",
            tokenEndpoint = "https://oauth2.googleapis.com/token",
            scopes = listOf(
                "https://www.googleapis.com/auth/tasks",
                "https://www.googleapis.com/auth/userinfo.email",
            )
        )
    }

    suspend fun authenticate(config: OAuthConfig): Result<TokenResponse> = withContext(Dispatchers.IO) {
        try {
            // Start callback server
            val port = callbackServer.findAvailablePort()
            val redirectUri = callbackServer.start(port)

            // Build authorization URL
            val authUrl = buildAuthorizationUrl(config, redirectUri)

            // Open browser
            if (Desktop.isDesktopSupported()) {
                Desktop.getDesktop().browse(URI(authUrl))
            } else {
                callbackServer.stop()
                return@withContext Result.failure(Exception("Desktop browsing not supported"))
            }

            // Wait for callback
            val result = callbackServer.waitForCallback()
            callbackServer.stop()

            if (result == null) {
                return@withContext Result.failure(Exception("Authentication timed out"))
            }

            if (!result.isSuccess) {
                return@withContext Result.failure(
                    Exception(result.errorDescription ?: result.error ?: "Authentication failed")
                )
            }

            // Exchange code for tokens
            val tokenResponse = exchangeCodeForTokens(config, result.code!!, redirectUri)
            Result.success(tokenResponse)
        } catch (e: Exception) {
            callbackServer.stop()
            Result.failure(e)
        }
    }

    suspend fun refreshToken(config: OAuthConfig, refreshToken: String): Result<TokenResponse> =
        withContext(Dispatchers.IO) {
            try {
                val formBody = FormBody.Builder()
                    .add("client_id", config.clientId)
                    .add("client_secret", config.clientSecret)
                    .add("refresh_token", refreshToken)
                    .add("grant_type", "refresh_token")
                    .build()

                val request = Request.Builder()
                    .url(config.tokenEndpoint)
                    .post(formBody)
                    .build()

                val response = httpClient.newCall(request).execute()
                val body = response.body?.string() ?: throw Exception("Empty response")

                if (!response.isSuccessful) {
                    throw Exception("Token refresh failed: $body")
                }

                val jsonObj = json.parseToJsonElement(body).jsonObject
                Result.success(
                    TokenResponse(
                        accessToken = jsonObj["access_token"]?.jsonPrimitive?.content ?: throw Exception("Missing access_token"),
                        refreshToken = jsonObj["refresh_token"]?.jsonPrimitive?.content ?: refreshToken,
                        expiresIn = jsonObj["expires_in"]?.jsonPrimitive?.long ?: 3600,
                        tokenType = jsonObj["token_type"]?.jsonPrimitive?.content ?: "Bearer",
                    )
                )
            } catch (e: Exception) {
                Result.failure(e)
            }
        }

    private fun buildAuthorizationUrl(config: OAuthConfig, redirectUri: String): String {
        val params = mapOf(
            "client_id" to config.clientId,
            "redirect_uri" to redirectUri,
            "response_type" to "code",
            "scope" to config.scopes.joinToString(" "),
            "access_type" to "offline",
            "prompt" to "consent",
        )

        val queryString = params.entries.joinToString("&") { (key, value) ->
            "${URLEncoder.encode(key, "UTF-8")}=${URLEncoder.encode(value, "UTF-8")}"
        }

        return "${config.authorizationEndpoint}?$queryString"
    }

    private fun exchangeCodeForTokens(
        config: OAuthConfig,
        code: String,
        redirectUri: String
    ): TokenResponse {
        val formBody = FormBody.Builder()
            .add("client_id", config.clientId)
            .add("client_secret", config.clientSecret)
            .add("code", code)
            .add("redirect_uri", redirectUri)
            .add("grant_type", "authorization_code")
            .build()

        val request = Request.Builder()
            .url(config.tokenEndpoint)
            .post(formBody)
            .build()

        val response = httpClient.newCall(request).execute()
        val body = response.body?.string() ?: throw Exception("Empty response")

        if (!response.isSuccessful) {
            throw Exception("Token exchange failed: $body")
        }

        val jsonObj = json.parseToJsonElement(body).jsonObject
        return TokenResponse(
            accessToken = jsonObj["access_token"]?.jsonPrimitive?.content ?: throw Exception("Missing access_token"),
            refreshToken = jsonObj["refresh_token"]?.jsonPrimitive?.content,
            expiresIn = jsonObj["expires_in"]?.jsonPrimitive?.long ?: 3600,
            tokenType = jsonObj["token_type"]?.jsonPrimitive?.content ?: "Bearer",
        )
    }
}
