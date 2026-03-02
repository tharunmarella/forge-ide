package com.forge.plugin.api

import com.forge.plugin.ui.ForgeUIService
import com.google.gson.Gson
import com.google.gson.JsonObject
import com.intellij.openapi.components.Service
import com.intellij.openapi.project.Project
import okhttp3.*
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.sse.EventSource
import okhttp3.sse.EventSourceListener
import okhttp3.sse.EventSources
import java.io.IOException
import java.util.concurrent.TimeUnit

@Service(Service.Level.PROJECT)
class ForgeApiService(private val project: Project) {
    private val client = OkHttpClient.Builder()
        .connectTimeout(30, TimeUnit.SECONDS)
        .readTimeout(0, TimeUnit.SECONDS) // For streaming
        .writeTimeout(60, TimeUnit.SECONDS)
        .build()
    
    private val gson = Gson()
    
    // Default to the streaming endpoint
    private var backendUrl = "https://forge-search-production.up.railway.app/chat/stream"
    private var conversationId: String = java.util.UUID.randomUUID().toString()
    
    fun setBackendUrl(url: String) {
        var adjustedUrl = url
        if (adjustedUrl.endsWith("/api/agent")) {
            adjustedUrl = adjustedUrl.replace("/api/agent", "/chat/stream")
        }
        this.backendUrl = adjustedUrl
    }

    fun getBackendUrl(): String = backendUrl

    fun sendMessage(message: String) {
        processMessage(message, null)
    }

    fun clearChat() {
        conversationId = java.util.UUID.randomUUID().toString()
    }

    private fun getForgeToken(): String {
        try {
            val userHome = System.getProperty("user.home")
            // Try agent config dir first
            val agentConfig = java.io.File(userHome, ".config/forge-ide/forge-auth.json")
            if (agentConfig.exists()) {
                val content = agentConfig.readText()
                val json = gson.fromJson(content, JsonObject::class.java)
                return json.get("token")?.asString ?: ""
            }
            
            // Try mac specific lapce config
            val macConfig = java.io.File(userHome, "Library/Application Support/dev.lapce.Lapce-Nightly/forge-auth.json")
            if (macConfig.exists()) {
                val content = macConfig.readText()
                val json = gson.fromJson(content, JsonObject::class.java)
                return json.get("token")?.asString ?: ""
            }
            
            // Try linux specific lapce config
            val linuxConfig = java.io.File(userHome, ".config/dev.lapce.Lapce-Nightly/forge-auth.json")
            if (linuxConfig.exists()) {
                val content = linuxConfig.readText()
                val json = gson.fromJson(content, JsonObject::class.java)
                return json.get("token")?.asString ?: ""
            }
        } catch (e: Exception) {
            println("Failed to read token: ${e.message}")
        }
        return ""
    }

    private fun processMessage(message: String?, toolResults: List<Map<String, Any>>?) {
        val uiService = project.getService(ForgeUIService::class.java)
        
        val json = JsonObject()
        json.addProperty("workspace_id", project.name)
        json.addProperty("conversation_id", conversationId)
        
        if (message != null) {
            json.addProperty("question", message)
        }
        
        if (toolResults != null) {
            json.add("tool_results", gson.toJsonTree(toolResults))
        }
        
        val bodyStr = json.toString()
        println("Sending payload to backend: $bodyStr")
        val body = bodyStr.toRequestBody("application/json".toMediaType())
        val requestBuilder = Request.Builder()
            .url(backendUrl)
            .post(body)
            .header("Accept", "text/event-stream")
            
        val token = getForgeToken()
        if (token.isNotEmpty()) {
            requestBuilder.header("Authorization", "Bearer $token")
        }
        
        val request = requestBuilder.build()
        println("Sending request to $backendUrl with token length ${token.length}")

        val eventSourceListener = object : EventSourceListener() {
            override fun onOpen(eventSource: EventSource, response: Response) {
                println("SSE Connection opened. Response: ${response.code}")
            }

            override fun onEvent(eventSource: EventSource, id: String?, type: String?, data: String) {
                println("Received SSE Event: type=$type, data=$data")
                val sseEvent = SseParser.parse(type, data, gson)
                if (sseEvent == null) return

                when (sseEvent) {
                    is SseEvent.TextDelta -> {
                        uiService.postMessage(gson.toJson(mapOf("type" to "text_delta", "content" to sseEvent.text)))
                    }
                    is SseEvent.Thinking -> {
                        // For now we map this to thinking_start or thinking_delta based on UI needs
                        uiService.postMessage(gson.toJson(mapOf("type" to "thinking_start")))
                        uiService.postMessage(gson.toJson(mapOf("type" to "thinking_delta", "content" to sseEvent.message)))
                    }
                    is SseEvent.ToolStart -> {
                        uiService.postMessage(gson.toJson(mapOf(
                            "type" to "tool_start",
                            "id" to sseEvent.id,
                            "name" to sseEvent.name,
                            "input" to sseEvent.input
                        )))
                    }
                    is SseEvent.ToolEnd -> {
                        uiService.postMessage(gson.toJson(mapOf(
                            "type" to "tool_end",
                            "id" to sseEvent.id,
                            "output" to sseEvent.resultSummary
                        )))
                    }
                    is SseEvent.Plan -> {
                        uiService.postMessage(gson.toJson(mapOf("type" to "plan_update", "steps" to sseEvent.steps)))
                    }
                    is SseEvent.RequiresAction -> {
                        val results = mutableListOf<Map<String, Any>>()
                        for (toolCall in sseEvent.toolCalls) {
                            val args = toolCall.args ?: JsonObject()
                            
                            val toolResultStr = ToolExecutor.execute(project, toolCall.name, args)
                            
                            var success = false
                            var outputStr = toolResultStr
                            
                            try {
                                val json = gson.fromJson(toolResultStr, JsonObject::class.java)
                                success = json.get("success")?.asBoolean ?: false
                                if (json.has("result")) {
                                    val resElem = json.get("result")
                                    outputStr = if (resElem.isJsonPrimitive && resElem.asJsonPrimitive.isString) {
                                        resElem.asString
                                    } else {
                                        resElem.toString()
                                    }
                                } else if (json.has("error")) {
                                    outputStr = json.get("error").asString
                                }
                            } catch (e: Exception) {
                                // If it's not JSON or failed to parse, leave as is
                            }
                            
                            results.add(mapOf(
                                "call_id" to toolCall.id, 
                                "output" to outputStr,
                                "success" to success
                            ))
                        }
                        // Close this stream and start a new one with results
                        eventSource.cancel()
                        processMessage(null, results)
                    }
                    is SseEvent.Error -> {
                        uiService.postMessage(gson.toJson(mapOf("type" to "error", "message" to sseEvent.message)))
                    }
                    is SseEvent.Done -> {
                        uiService.postMessage(gson.toJson(mapOf("type" to "done")))
                        eventSource.cancel()
                    }
                }
            }

            override fun onClosed(eventSource: EventSource) {
                println("SSE Connection closed")
            }

            override fun onFailure(eventSource: EventSource, t: Throwable?, response: Response?) {
                println("SSE Connection failed: ${t?.message}, response code: ${response?.code}")
                uiService.postMessage(gson.toJson(mapOf("type" to "error", "message" to "Connection failed: ${t?.message}")))
            }
        }

        EventSources.createFactory(client).newEventSource(request, eventSourceListener)
    }
}
