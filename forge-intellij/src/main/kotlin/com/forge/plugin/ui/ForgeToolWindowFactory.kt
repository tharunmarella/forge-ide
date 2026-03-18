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
import java.util.concurrent.atomic.AtomicBoolean

private val LOG = Logger.getInstance(ForgeToolWindowFactory::class.java)

class ForgeToolWindowFactory : ToolWindowFactory {

    override fun createToolWindowContent(project: Project, toolWindow: ToolWindow) {
        val browser = JBCefBrowser()
        val uiService = project.getService(ForgeUIService::class.java)
        uiService.setBrowser(browser)

        // Dispose the browser when the project closes — prevents a process leak
        Disposer.register(project as Disposable, Disposable {
            browser.dispose()
        })

        // Load the bundled HTML (vendor/ assets are local — works offline)
        val htmlContent = javaClass.classLoader
            .getResourceAsStream("webview/index.html")
            ?.bufferedReader()?.use { it.readText() }
        if (htmlContent != null) {
            browser.loadHTML(htmlContent, "http://forge-local/index.html")
        } else {
            LOG.error("Could not load webview/index.html from resources")
        }

        // JS → Kotlin query bridge
        val jsQuery = JBCefJSQuery.create(browser as JBCefBrowser)
        val gson = Gson()
        jsQuery.addHandler { message: String ->
            try {
                val json = gson.fromJson(message, JsonObject::class.java)
                when (json.get("action")?.asString) {
                    "chat" -> {
                        val text = json.get("text")?.asString ?: ""
                        val apiService = project.getService(com.forge.plugin.api.ForgeApiService::class.java)
                        apiService.sendMessage(text)
                    }
                    "stop" -> {
                        val apiService = project.getService(com.forge.plugin.api.ForgeApiService::class.java)
                        apiService.cancelStream()
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

        // On page load: inject bridge + check auth on a pooled thread (not the CEF IO thread)
        browser.jbCefClient.addLoadHandler(object : CefLoadHandlerAdapter() {
            override fun onLoadEnd(cefBrowser: CefBrowser, frame: CefFrame, httpStatusCode: Int) {
                val injectJs = """
                    window.sendActionToKotlin = function(msg) {
                        ${jsQuery.inject("msg")}
                    };
                """.trimIndent()
                cefBrowser.executeJavaScript(injectJs, cefBrowser.url, 0)

                // Inject IDE theme colors so the webview respects the active theme
                injectTheme(cefBrowser)

                // File I/O must not happen on the CEF IO thread — use the platform pool
                ApplicationManager.getApplication().executeOnPooledThread {
                    checkAuthentication(uiService)
                }
            }
        }, browser.cefBrowser)

        val content = ContentFactory.getInstance().createContent(browser.component, "", false)
        toolWindow.contentManager.addContent(content)
    }

    /** Inject IDE colors into the webview as CSS variables. */
    private fun injectTheme(cefBrowser: CefBrowser) {
        val isDark = com.intellij.ide.ui.LafManager.getInstance().currentLookAndFeel
            ?.name?.contains("dark", ignoreCase = true) != false
        val bg     = com.intellij.util.ui.UIUtil.getPanelBackground()
        val fg     = com.intellij.util.ui.UIUtil.getLabelForeground()
        val border = com.intellij.ui.JBColor.border()

        fun rgb(c: java.awt.Color) = "rgb(${c.red},${c.green},${c.blue})"

        val varsJson = buildString {
            append("{")
            append("\"bg-primary\":\"${rgb(bg)}\",")
            append("\"bg-secondary\":\"${rgb(bg.darker())}\",")
            append("\"bg-tertiary\":\"${rgb(bg.darker().darker())}\",")
            append("\"text-primary\":\"${rgb(fg)}\",")
            append("\"border-color\":\"${rgb(border)}\",")
            append("\"code-bg\":\"${if (isDark) "#1e1f22" else "#f4f4f4"}\",")
            append("\"code-border\":\"${rgb(border)}\"")
            append("}")
        }
        cefBrowser.executeJavaScript(
            "if(window.applyTheme) window.applyTheme($varsJson);",
            cefBrowser.url, 0
        )
    }

    private fun checkAuthentication(uiService: ForgeUIService) {
        val userHome = System.getProperty("user.home")
        val authFiles = listOf(
            java.io.File(userHome, ".config/forge-ide/forge-auth.json"),
            java.io.File(userHome, "Library/Application Support/dev.lapce.Lapce-Nightly/forge-auth.json"),
            java.io.File(userHome, ".config/dev.lapce.Lapce-Nightly/forge-auth.json")
        )
        val isAuthenticated = authFiles.any { it.exists() }
        uiService.postMessage(Gson().toJson(mapOf(
            "type" to "auth_status",
            "show_login" to !isAuthenticated,
            "message" to "Sign in to continue"
        )))
    }

    private fun startOAuthFlow(project: Project, provider: String, uiService: ForgeUIService) {
        val sessionId = java.util.UUID.randomUUID().toString()
        val baseUrl = System.getenv("FORGE_SEARCH_URL")
            ?: "https://forge-search-production.up.railway.app"
        val authUrl = "$baseUrl/auth/$provider?state=poll-$sessionId"
        val pollUrl = "$baseUrl/auth/poll/$sessionId"

        try {
            java.awt.Desktop.getDesktop().browse(java.net.URI(authUrl))
        } catch (e: Exception) {
            // Use IntelliJ's dialog API (with proper parent) instead of JOptionPane(null)
            ApplicationManager.getApplication().invokeLater {
                Messages.showErrorDialog(project, "Failed to open browser: ${e.message}", "Authentication Error")
            }
            return
        }

        uiService.postMessage(Gson().toJson(mapOf(
            "type" to "auth_status",
            "show_login" to true,
            "message" to "Waiting for authentication… (complete sign-in in your browser)"
        )))

        // Use the platform thread pool (not raw Thread) so the IDE can cancel on shutdown
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
                            val token = json.get("token")?.asString ?: ""
                            val email = json.get("email")?.asString ?: ""
                            val name  = json.get("name")?.asString  ?: ""
                            saveAuthToken(token, email, name)
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
                    LOG.debug("OAuth poll attempt $attempts failed: ${e.message}")
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
        val configDir = java.io.File(System.getProperty("user.home"), ".config/forge-ide")
        configDir.mkdirs()
        val authData = mapOf("token" to token, "email" to email, "name" to name)
        java.io.File(configDir, "forge-auth.json")
            .writeText(Gson().newBuilder().setPrettyPrinting().create().toJson(authData))
    }

    override fun isApplicable(project: Project): Boolean =
        com.intellij.ui.jcef.JBCefApp.isSupported()
}
