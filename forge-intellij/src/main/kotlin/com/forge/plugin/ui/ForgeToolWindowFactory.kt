package com.forge.plugin.ui

import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.Messages
import com.intellij.openapi.util.Disposer
import com.intellij.openapi.wm.ToolWindow
import com.intellij.openapi.wm.ToolWindowFactory
import com.intellij.ui.content.ContentFactory
import com.intellij.ui.jcef.JBCefBrowser
import com.intellij.ui.jcef.JBCefJSQuery
import org.cef.browser.CefBrowser
import org.cef.browser.CefFrame
import org.cef.handler.CefLoadHandlerAdapter
import com.google.gson.Gson
import com.google.gson.JsonObject
import java.io.File
import java.util.concurrent.atomic.AtomicBoolean

private val LOG = Logger.getInstance(ForgeToolWindowFactory::class.java)

class ForgeToolWindowFactory : ToolWindowFactory {

    override fun createToolWindowContent(project: Project, toolWindow: ToolWindow) {
        val browser = JBCefBrowser()
        val uiService = project.getService(ForgeUIService::class.java)
        uiService.setBrowser(browser)

        // Dispose the browser when the project closes to prevent resource leaks
        Disposer.register(project as Disposable, Disposable { browser.dispose() })

        // Extract webview assets to a temp dir so relative paths (vendor/*.js)
        // resolve correctly over file:// — loadHTML with a virtual origin breaks them
        val webviewDir = extractWebviewResources()
        if (webviewDir != null) {
            browser.loadURL("file://${webviewDir.absolutePath}/index.html")
        } else {
            LOG.error("Could not extract webview resources — Forge AI panel will be blank")
        }

        // JS to Kotlin bridge
        val jsQuery = JBCefJSQuery.create(browser as JBCefBrowser)
        val gson = Gson()
        jsQuery.addHandler { message: String ->
            try {
                val json = gson.fromJson(message, JsonObject::class.java)
                when (json.get("action")?.asString) {
                    "chat" -> {
                        val text = json.get("text")?.asString ?: ""
                        project.getService(com.forge.plugin.api.ForgeApiService::class.java)
                            .sendMessage(text)
                    }
                    "stop" -> {
                        project.getService(com.forge.plugin.api.ForgeApiService::class.java)
                            .cancelStream()
                    }
                    "auth" -> {
                        val provider = json.get("provider")?.asString ?: "github"
                        startOAuthFlow(project, provider, uiService)
                    }
                }
            } catch (e: Exception) {
                LOG.warn("Error parsing JS message: ${e.message}")
            }
            JBCefJSQuery.Response("OK")
        }

        // On page load: inject bridge function and check auth on a pooled thread
        browser.jbCefClient.addLoadHandler(object : CefLoadHandlerAdapter() {
            override fun onLoadEnd(cefBrowser: CefBrowser, frame: CefFrame, httpStatusCode: Int) {
                val injectJs = "window.sendActionToKotlin = function(msg) { ${jsQuery.inject("msg")} };"
                cefBrowser.executeJavaScript(injectJs, cefBrowser.url, 0)
                injectTheme(cefBrowser)
                ApplicationManager.getApplication().executeOnPooledThread {
                    checkAuthentication(uiService)
                }
            }
        }, browser.cefBrowser)

        val content = ContentFactory.getInstance().createContent(browser.component, "", false)
        toolWindow.contentManager.addContent(content)
    }

    // ── Private helpers ──────────────────────────────────────────────────────

    private fun extractWebviewResources(): File? {
        return try {
            val tempDir = File(System.getProperty("java.io.tmpdir"), "forge-webview")
            val resources = listOf(
                "webview/index.html",
                "webview/vendor/marked.min.js",
                "webview/vendor/highlight.min.js",
                "webview/vendor/github-dark.min.css",
                "webview/vendor/mermaid.min.js"
            )
            for (resource in resources) {
                val stream = javaClass.classLoader.getResourceAsStream(resource) ?: continue
                val target = File(tempDir, resource.removePrefix("webview/"))
                target.parentFile.mkdirs()
                stream.use { input -> target.outputStream().use { out -> input.copyTo(out) } }
            }
            tempDir
        } catch (e: Exception) {
            LOG.error("Failed to extract webview resources", e)
            null
        }
    }

    private fun injectTheme(cefBrowser: CefBrowser) {
        val laf = com.intellij.ide.ui.LafManager.getInstance().currentLookAndFeel
        val bg     = com.intellij.util.ui.UIUtil.getPanelBackground()
        val fg     = com.intellij.util.ui.UIUtil.getLabelForeground()
        val border = com.intellij.ui.JBColor.border()

        fun rgb(c: java.awt.Color) = "rgb(${c.red},${c.green},${c.blue})"

        val vars = mapOf(
            "background"       to rgb(bg),
            "foreground"       to rgb(fg),
            "muted"            to rgb(bg.darker()),
            "muted-foreground" to rgb(fg.darker()),
            "border"           to rgb(border),
            "input"            to rgb(bg.darker()),
            "ring"             to rgb(border),
            "primary"          to rgb(fg),
            "primary-foreground" to rgb(bg)
        )
        val json = Gson().toJson(vars)
        cefBrowser.executeJavaScript(
            "if(window.applyTheme) window.applyTheme($json);",
            cefBrowser.url, 0
        )
    }

    private fun checkAuthentication(uiService: ForgeUIService) {
        val home = System.getProperty("user.home")
        val authFiles = listOf(
            File(home, ".config/forge-ide/forge-auth.json"),
            File(home, "Library/Application Support/dev.lapce.Lapce-Nightly/forge-auth.json"),
            File(home, ".config/dev.lapce.Lapce-Nightly/forge-auth.json")
        )
        val isAuthenticated = authFiles.any { it.exists() && !isTokenExpired(it) }
        uiService.postMessage(Gson().toJson(mapOf(
            "type"       to "auth_status",
            "show_login" to !isAuthenticated,
            "message"    to if (isAuthenticated) "" else "Your session has expired. Please sign in again."
        )))
    }

    /** Decode the JWT payload and check the 'exp' claim against the current time. */
    private fun isTokenExpired(authFile: File): Boolean {
        return try {
            val json = Gson().fromJson(authFile.readText(), JsonObject::class.java)
            val token = json.get("token")?.asString ?: return true
            val parts = token.split(".")
            if (parts.size < 2) return true
            // JWT payload is base64url-encoded (no padding)
            val payload = String(
                java.util.Base64.getUrlDecoder().decode(
                    parts[1].padEnd((parts[1].length + 3) / 4 * 4, '=')
                )
            )
            val payloadJson = Gson().fromJson(payload, JsonObject::class.java)
            val exp = payloadJson.get("exp")?.asLong ?: return true
            val nowSec = System.currentTimeMillis() / 1000
            val expired = nowSec >= exp
            if (expired) LOG.info("Auth token expired (exp=$exp, now=$nowSec) — showing login")
            expired
        } catch (e: Exception) {
            LOG.warn("Could not decode JWT, treating as expired: ${e.message}")
            true
        }
    }

    private fun startOAuthFlow(project: Project, provider: String, uiService: ForgeUIService) {
        val sessionId = java.util.UUID.randomUUID().toString()
        val production = "https://forge-search-production.up.railway.app"
        val envUrl = System.getenv("FORGE_SEARCH_URL")
        val baseUrl = if (!envUrl.isNullOrBlank() && !envUrl.contains("your-backend") && !envUrl.contains("placeholder"))
            envUrl else production
        val authUrl = "$baseUrl/auth/$provider?state=poll-$sessionId"
        val pollUrl = "$baseUrl/auth/poll/$sessionId"

        try {
            java.awt.Desktop.getDesktop().browse(java.net.URI(authUrl))
        } catch (e: Exception) {
            ApplicationManager.getApplication().invokeLater {
                Messages.showErrorDialog(project, "Failed to open browser: ${e.message}", "Auth Error")
            }
            return
        }

        uiService.postMessage(Gson().toJson(mapOf(
            "type"       to "auth_status",
            "show_login" to true,
            "message"    to "Waiting for authentication in browser..."
        )))

        val cancelled = AtomicBoolean(false)
        ApplicationManager.getApplication().executeOnPooledThread {
            var attempts = 0
            var authenticated = false
            while (attempts < 60 && !authenticated && !cancelled.get()
                && !ApplicationManager.getApplication().isDisposed) {
                Thread.sleep(5000)
                attempts++
                try {
                    val response = com.intellij.util.io.HttpRequests.request(pollUrl).readString()
                    val json = Gson().fromJson(response, JsonObject::class.java)
                    when (json.get("status")?.asString) {
                        "success" -> {
                            saveAuthToken(
                                json.get("token")?.asString ?: "",
                                json.get("email")?.asString ?: "",
                                json.get("name")?.asString  ?: ""
                            )
                            authenticated = true
                            uiService.postMessage(Gson().toJson(mapOf(
                                "type" to "auth_status", "show_login" to false
                            )))
                        }
                        "expired" -> {
                            uiService.postMessage(Gson().toJson(mapOf(
                                "type" to "auth_status", "show_login" to true,
                                "message" to "Session expired. Please try again."
                            )))
                            return@executeOnPooledThread
                        }
                    }
                } catch (e: Exception) {
                    LOG.debug("OAuth poll $attempts failed: ${e.message}")
                }
            }
            if (!authenticated && !cancelled.get()) {
                uiService.postMessage(Gson().toJson(mapOf(
                    "type" to "auth_status", "show_login" to true,
                    "message" to "Authentication timed out. Please try again."
                )))
            }
        }
    }

    private fun saveAuthToken(token: String, email: String, name: String) {
        val configDir = File(System.getProperty("user.home"), ".config/forge-ide")
        configDir.mkdirs()
        File(configDir, "forge-auth.json").writeText(
            Gson().newBuilder().setPrettyPrinting().create()
                .toJson(mapOf("token" to token, "email" to email, "name" to name))
        )
    }

    override fun isApplicable(project: Project): Boolean =
        com.intellij.ui.jcef.JBCefApp.isSupported()
}
