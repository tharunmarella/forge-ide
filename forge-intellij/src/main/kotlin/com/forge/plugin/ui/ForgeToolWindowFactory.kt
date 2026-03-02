package com.forge.plugin.ui

import com.intellij.openapi.project.Project
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
import javax.swing.SwingUtilities
import javax.swing.JOptionPane

class ForgeToolWindowFactory : ToolWindowFactory {

    override fun createToolWindowContent(project: Project, toolWindow: ToolWindow) {
        val browser = JBCefBrowser()
        val uiService = project.getService(ForgeUIService::class.java)
        uiService.setBrowser(browser)

        // Load our modern web UI
        val htmlUrl = javaClass.classLoader.getResource("webview/index.html")?.toExternalForm()
            ?: "<html><body><h1>Error loading UI</h1></body></html>"
            
        try {
            val htmlContent = javaClass.classLoader.getResourceAsStream("webview/index.html")?.bufferedReader()?.use { it.readText() }
            if (htmlContent != null) {
                browser.loadHTML(htmlContent, "http://forge-local/index.html")
            } else {
                browser.loadURL(htmlUrl)
            }
        } catch (e: Exception) {
            browser.loadURL(htmlUrl)
        }

        // Handle messages FROM Javascript TO Kotlin
        val jsQuery = JBCefJSQuery.create(browser as JBCefBrowser)
        jsQuery.addHandler { message: String ->
            val gson = Gson()
            try {
                val json = gson.fromJson(message, JsonObject::class.java)
                val action = json.get("action")?.asString
                
                when (action) {
                    "chat" -> {
                        val text = json.get("text")?.asString ?: ""
                        val apiService = project.getService(com.forge.plugin.api.ForgeApiService::class.java)
                        apiService.sendMessage(text)
                    }
                    "auth" -> {
                        val provider = json.get("provider")?.asString ?: "github"
                        startOAuthFlow(project, provider, uiService)
                    }
                }
            } catch (e: Exception) {
                println("Error parsing JS message: ${e.message}")
            }
            JBCefJSQuery.Response("OK")
        }

        // Inject the Kotlin handler into Javascript and check auth state once loaded
        browser.jbCefClient.addLoadHandler(object : CefLoadHandlerAdapter() {
            override fun onLoadEnd(cefBrowser: CefBrowser, frame: CefFrame, httpStatusCode: Int) {
                val injectJs = """
                    window.sendActionToKotlin = function(msg) {
                        ${jsQuery.inject("msg")}
                    };
                """.trimIndent()
                cefBrowser.executeJavaScript(injectJs, cefBrowser.url, 0)
                
                // Check auth status right after loading
                checkAuthentication(uiService)
            }
        }, browser.cefBrowser)

        val contentFactory = ContentFactory.getInstance()
        val content = contentFactory.createContent(browser.component, "", false)
        toolWindow.contentManager.addContent(content)
    }

    private fun checkAuthentication(uiService: ForgeUIService) {
        val userHome = System.getProperty("user.home")
        val authFiles = listOf(
            java.io.File(userHome, ".config/forge-ide/forge-auth.json"),
            java.io.File(userHome, "Library/Application Support/dev.lapce.Lapce-Nightly/forge-auth.json"),
            java.io.File(userHome, ".config/dev.lapce.Lapce-Nightly/forge-auth.json")
        )
        
        val isAuthenticated = authFiles.any { it.exists() }
        
        val status = mapOf(
            "type" to "auth_status",
            "show_login" to !isAuthenticated,
            "message" to "Sign in to continue"
        )
        
        uiService.postMessage(Gson().toJson(status))
    }

    private fun startOAuthFlow(project: Project, provider: String, uiService: ForgeUIService) {
        val sessionId = java.util.UUID.randomUUID().toString()
        val baseUrl = System.getenv("FORGE_SEARCH_URL") 
            ?: "https://forge-search-production.up.railway.app"
        val authUrl = "$baseUrl/auth/$provider?state=poll-$sessionId"
        val pollUrl = "$baseUrl/auth/poll/$sessionId"
        
        // Open browser
        try {
            java.awt.Desktop.getDesktop().browse(java.net.URI(authUrl))
        } catch (e: Exception) {
            SwingUtilities.invokeLater {
                JOptionPane.showMessageDialog(null, "Failed to open browser: ${e.message}", "Error", JOptionPane.ERROR_MESSAGE)
            }
            return
        }
        
        // Update UI to show waiting state
        val waitStatus = mapOf(
            "type" to "auth_status",
            "show_login" to true,
            "message" to "Waiting for authentication... (complete sign-in in browser)"
        )
        uiService.postMessage(Gson().toJson(waitStatus))
        
        // Start polling in background thread
        Thread {
            var attempts = 0
            var authenticated = false
            
            while (attempts < 60 && !authenticated) {
                Thread.sleep(5000)
                attempts++
                
                try {
                    val response = com.intellij.util.io.HttpRequests.request(pollUrl).readString()
                    val json = Gson().fromJson(response, JsonObject::class.java)
                    val status = json.get("status")?.asString ?: "pending"
                    
                    when (status) {
                        "success" -> {
                            val token = json.get("token")?.asString ?: ""
                            val email = json.get("email")?.asString ?: ""
                            val name = json.get("name")?.asString ?: ""
                            
                            saveAuthToken(token, email, name)
                            authenticated = true
                            
                            val successStatus = mapOf(
                                "type" to "auth_status",
                                "show_login" to false
                            )
                            uiService.postMessage(Gson().toJson(successStatus))
                        }
                        "expired" -> {
                            val expiredStatus = mapOf(
                                "type" to "auth_status",
                                "show_login" to true,
                                "message" to "Session expired. Please try again."
                            )
                            uiService.postMessage(Gson().toJson(expiredStatus))
                            break
                        }
                    }
                } catch (e: Exception) {
                    // Continue polling
                }
            }
            
            if (!authenticated && attempts >= 60) {
                val timeoutStatus = mapOf(
                    "type" to "auth_status",
                    "show_login" to true,
                    "message" to "Authentication timed out. Please try again."
                )
                uiService.postMessage(Gson().toJson(timeoutStatus))
            }
        }.start()
    }

    private fun saveAuthToken(token: String, email: String, name: String) {
        val userHome = System.getProperty("user.home")
        val configDir = java.io.File(userHome, ".config/forge-ide")
        configDir.mkdirs()
        
        val authFile = java.io.File(configDir, "forge-auth.json")
        val authData = mapOf(
            "token" to token,
            "email" to email,
            "name" to name
        )
        authFile.writeText(Gson().newBuilder().setPrettyPrinting().create().toJson(authData))
    }

    override fun isApplicable(project: Project): Boolean {
        return com.intellij.ui.jcef.JBCefApp.isSupported()
    }
}
