package org.tasks.desktop.auth

import com.sun.net.httpserver.HttpExchange
import com.sun.net.httpserver.HttpHandler
import com.sun.net.httpserver.HttpServer
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.withTimeoutOrNull
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.util.concurrent.Executors

class OAuthCallbackServer {
    private var server: HttpServer? = null
    private var callbackDeferred: CompletableDeferred<OAuthResult>? = null

    data class OAuthResult(
        val code: String?,
        val error: String?,
        val errorDescription: String?
    ) {
        val isSuccess: Boolean get() = code != null && error == null
    }

    fun findAvailablePort(): Int {
        ServerSocket(0).use { socket ->
            return socket.localPort
        }
    }

    fun start(port: Int): String {
        val deferred = CompletableDeferred<OAuthResult>()
        callbackDeferred = deferred

        server = HttpServer.create(InetSocketAddress(port), 0).apply {
            createContext("/callback", CallbackHandler(deferred))
            executor = Executors.newSingleThreadExecutor()
            start()
        }

        return "http://localhost:$port/callback"
    }

    suspend fun waitForCallback(timeoutMs: Long = 300_000): OAuthResult? {
        return withTimeoutOrNull(timeoutMs) {
            callbackDeferred?.await()
        }
    }

    fun stop() {
        server?.stop(0)
        server = null
        callbackDeferred = null
    }

    private class CallbackHandler(
        private val deferred: CompletableDeferred<OAuthResult>
    ) : HttpHandler {

        override fun handle(exchange: HttpExchange) {
            try {
                val query = exchange.requestURI.query ?: ""
                val params = parseQueryString(query)

                val result = OAuthResult(
                    code = params["code"],
                    error = params["error"],
                    errorDescription = params["error_description"]
                )

                // Send response to browser
                val responseBody = if (result.isSuccess) {
                    """
                    <!DOCTYPE html>
                    <html>
                    <head>
                        <title>Authentication Successful</title>
                        <style>
                            body {
                                font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
                                display: flex;
                                justify-content: center;
                                align-items: center;
                                height: 100vh;
                                margin: 0;
                                background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
                            }
                            .card {
                                background: white;
                                padding: 40px;
                                border-radius: 16px;
                                box-shadow: 0 10px 40px rgba(0,0,0,0.2);
                                text-align: center;
                            }
                            h1 { color: #22c55e; margin-bottom: 16px; }
                            p { color: #666; }
                        </style>
                    </head>
                    <body>
                        <div class="card">
                            <h1>Authentication Successful</h1>
                            <p>You can close this window and return to Tasks.</p>
                        </div>
                    </body>
                    </html>
                    """.trimIndent()
                } else {
                    """
                    <!DOCTYPE html>
                    <html>
                    <head>
                        <title>Authentication Failed</title>
                        <style>
                            body {
                                font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
                                display: flex;
                                justify-content: center;
                                align-items: center;
                                height: 100vh;
                                margin: 0;
                                background: linear-gradient(135deg, #f87171 0%, #ef4444 100%);
                            }
                            .card {
                                background: white;
                                padding: 40px;
                                border-radius: 16px;
                                box-shadow: 0 10px 40px rgba(0,0,0,0.2);
                                text-align: center;
                            }
                            h1 { color: #ef4444; margin-bottom: 16px; }
                            p { color: #666; }
                        </style>
                    </head>
                    <body>
                        <div class="card">
                            <h1>Authentication Failed</h1>
                            <p>${result.errorDescription ?: result.error ?: "Unknown error"}</p>
                        </div>
                    </body>
                    </html>
                    """.trimIndent()
                }

                exchange.responseHeaders.add("Content-Type", "text/html; charset=utf-8")
                exchange.sendResponseHeaders(200, responseBody.length.toLong())
                exchange.responseBody.use { os ->
                    os.write(responseBody.toByteArray())
                }

                deferred.complete(result)
            } catch (e: Exception) {
                deferred.completeExceptionally(e)
            }
        }

        private fun parseQueryString(query: String): Map<String, String> {
            return query.split("&")
                .filter { it.contains("=") }
                .associate {
                    val (key, value) = it.split("=", limit = 2)
                    java.net.URLDecoder.decode(key, "UTF-8") to
                            java.net.URLDecoder.decode(value, "UTF-8")
                }
        }
    }
}
