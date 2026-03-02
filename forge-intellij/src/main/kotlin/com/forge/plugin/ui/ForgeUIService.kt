package com.forge.plugin.ui

import com.intellij.openapi.components.Service
import com.intellij.openapi.project.Project
import com.intellij.ui.jcef.JBCefBrowser
import com.google.gson.Gson

@Service(Service.Level.PROJECT)
class ForgeUIService(val project: Project) {
    private var browser: JBCefBrowser? = null
    private val gson = Gson()

    fun setBrowser(browser: JBCefBrowser) {
        this.browser = browser
    }

    fun executeJavaScript(script: String) {
        browser?.cefBrowser?.executeJavaScript(script, browser?.cefBrowser?.url ?: "", 0)
    }

    fun postMessage(message: String) {
        // We use Base64 to safely pass JSON strings containing quotes/newlines into JS
        val base64 = java.util.Base64.getEncoder().encodeToString(message.toByteArray(Charsets.UTF_8))
        val script = """
            try { 
                const jsonStr = new TextDecoder().decode(Uint8Array.from(atob('$base64'), c => c.charCodeAt(0)));
                if (window.receiveMessageFromKotlin) {
                    window.receiveMessageFromKotlin(jsonStr);
                }
            } catch(e) { 
                console.error('Error decoding message from Kotlin:', e); 
            }
        """.trimIndent()
        executeJavaScript(script)
    }
    
    fun showDiagram(diagramCode: String, title: String?) {
        val event = mapOf(
            "type" to "show_diagram",
            "code" to diagramCode,
            "title" to (title ?: "")
        )
        postMessage(gson.toJson(event))
    }
    
    fun showCode(code: String, language: String) {
        val event = mapOf(
            "type" to "show_code",
            "code" to code,
            "language" to language
        )
        postMessage(gson.toJson(event))
    }
}
