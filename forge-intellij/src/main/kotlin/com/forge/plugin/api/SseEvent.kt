package com.forge.plugin.api

import com.google.gson.annotations.SerializedName

/**
 * Represents the different types of events received via Server-Sent Events (SSE)
 * from the /chat/stream endpoint.
 */
sealed class SseEvent {
    data class TextDelta(val text: String) : SseEvent()
    
    data class Thinking(val stepType: String, val message: String, val detail: String?) : SseEvent()

    data class ToolStart(val id: String, val name: String, val input: String) : SseEvent()
    data class ToolEnd(val id: String, val name: String, val resultSummary: String, val success: Boolean) : SseEvent()

    data class Plan(val steps: List<PlanStep>) : SseEvent()

    data class RequiresAction(
        @SerializedName("tool_calls") val toolCalls: List<BackendToolCall>
    ) : SseEvent()

    data class Error(val message: String) : SseEvent()
    object Done : SseEvent()
}

data class PlanStep(
    val number: Int,
    val description: String,
    val status: String // "pending", "in_progress", "done", "failed"
)

data class BackendToolCall(
    @SerializedName("id") val id: String,
    @SerializedName("name") val name: String,
    @SerializedName("args") val args: com.google.gson.JsonObject?
)

/**
 * Utility to parse raw SSE data strings into SseEvent objects.
 */
object SseParser {
    fun parse(eventName: String?, data: String, gson: com.google.gson.Gson): SseEvent? {
        return try {
            val json = if (data.trim() == "[DONE]") null else gson.fromJson(data, com.google.gson.JsonObject::class.java)
            
            when (eventName) {
                "text_delta" -> SseEvent.TextDelta(json?.get("text")?.asString ?: "")
                "thinking" -> SseEvent.Thinking(
                    json?.get("step_type")?.asString ?: "",
                    json?.get("message")?.asString ?: "",
                    json?.get("detail")?.asString
                )
                "tool_start" -> {
                    SseEvent.ToolStart(
                        json?.get("tool_call_id")?.asString ?: "",
                        json?.get("tool_name")?.asString ?: "",
                        json?.get("arguments")?.toString() ?: ""
                    )
                }
                "tool_end" -> {
                    SseEvent.ToolEnd(
                        json?.get("tool_call_id")?.asString ?: "",
                        json?.get("tool_name")?.asString ?: "",
                        json?.get("result_summary")?.asString ?: "",
                        json?.get("success")?.asBoolean ?: false
                    )
                }
                "plan" -> {
                    val stepsJson = json?.get("steps")?.asJsonArray
                    val steps = stepsJson?.map { 
                        gson.fromJson(it, PlanStep::class.java)
                    } ?: emptyList()
                    SseEvent.Plan(steps)
                }
                "requires_action" -> {
                    // Manual parsing to avoid Gson Field Naming policy issues if any
                    val toolCallsJson = json?.get("tool_calls")?.asJsonArray
                    val calls = mutableListOf<BackendToolCall>()
                    toolCallsJson?.forEach { elem ->
                        val callObj = elem.asJsonObject
                        calls.add(
                            BackendToolCall(
                                id = callObj.get("id")?.asString ?: "",
                                name = callObj.get("name")?.asString ?: "",
                                args = callObj.get("args")?.asJsonObject
                            )
                        )
                    }
                    SseEvent.RequiresAction(calls)
                }
                "error" -> SseEvent.Error(json?.get("error")?.asString ?: "Unknown error")
                "done" -> SseEvent.Done
                else -> {
                    if (data == "[DONE]") SseEvent.Done else null
                }
            }
        } catch (e: Exception) {
            println("Failed to parse SSE event: ${e.message} for data: $data")
            SseEvent.Error("Failed to parse event: ${e.message}")
        }
    }
}
