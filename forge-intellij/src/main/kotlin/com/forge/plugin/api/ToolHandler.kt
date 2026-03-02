package com.forge.plugin.api

import com.google.gson.Gson
import com.google.gson.JsonObject
import com.intellij.openapi.project.Project

interface ToolHandler {
    fun execute(project: Project, args: JsonObject): String
}

object ToolResult {
    private val gson = Gson()

    fun success(data: Any? = null): String {
        val result = mutableMapOf<String, Any?>("success" to true)
        if (data != null) {
            result["result"] = data
        }
        return gson.toJson(result)
    }

    fun error(message: String): String {
        return gson.toJson(mapOf(
            "success" to false,
            "error" to message
        ))
    }
}
