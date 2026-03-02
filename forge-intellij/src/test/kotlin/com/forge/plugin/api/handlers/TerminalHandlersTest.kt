package com.forge.plugin.api.handlers

import com.google.gson.JsonObject
import com.intellij.testFramework.fixtures.BasePlatformTestCase

class TerminalHandlersTest : BasePlatformTestCase() {

    fun testHandleExecuteCommand() {
        val args = JsonObject()
        // Use a simple command that works on all OS
        val command = if (System.getProperty("os.name").lowercase().contains("win")) {
            "echo Hello Forge"
        } else {
            "echo 'Hello Forge'"
        }
        args.addProperty("command", command)
        
        val resultJsonStr = TerminalHandlers.handleExecuteCommand(project, args)
        val resultObj = com.google.gson.JsonParser.parseString(resultJsonStr).asJsonObject
        
        assertTrue(resultObj.get("success").asBoolean)
        
        val resultData = resultObj.get("result").asJsonObject
        assertEquals(0, resultData.get("exit_code").asInt)
        assertTrue(resultData.get("stdout").asString.contains("Hello Forge"))
    }

    fun testHandleCheckPort() {
        // Find an open port by binding to 0
        val serverSocket = java.net.ServerSocket(0)
        val port = serverSocket.localPort
        
        val args = JsonObject()
        args.addProperty("port", port)
        
        val resultJsonStr = TerminalHandlers.handleCheckPort(project, args)
        val resultObj = com.google.gson.JsonParser.parseString(resultJsonStr).asJsonObject
        
        assertTrue(resultObj.get("success").asBoolean)
        val resultData = resultObj.get("result").asJsonObject
        assertEquals("in_use", resultData.get("status").asString)
        
        serverSocket.close()
        
        // Now it should be available
        val resultJsonStr2 = TerminalHandlers.handleCheckPort(project, args)
        val resultObj2 = com.google.gson.JsonParser.parseString(resultJsonStr2).asJsonObject
        
        assertTrue(resultObj2.get("success").asBoolean)
        val resultData2 = resultObj2.get("result").asJsonObject
        assertEquals("available", resultData2.get("status").asString)
    }

    fun testHandleFetchWebpage() {
        val args = JsonObject()
        args.addProperty("url", "https://example.com")
        
        val resultJsonStr = TerminalHandlers.handleFetchWebpage(project, args)
        val resultObj = com.google.gson.JsonParser.parseString(resultJsonStr).asJsonObject
        
        assertTrue(resultObj.get("success").asBoolean)
        val resultData = resultObj.get("result").asJsonObject
        assertTrue(resultData.get("content").asString.contains("Example Domain"))
    fun testBackgroundProcessFlow() {
        // 1. Start background process
        val startArgs = JsonObject()
        val command = if (System.getProperty("os.name").lowercase().contains("win")) {
            "ping 127.0.0.1 -n 3"
        } else {
            "sleep 2 && echo 'Done sleeping'"
        }
        startArgs.addProperty("command", command)
        
        val startJsonStr = TerminalHandlers.handleExecuteBackground(project, startArgs)
        val startObj = com.google.gson.JsonParser.parseString(startJsonStr).asJsonObject
        assertTrue(startObj.get("success").asBoolean)
        
        val pid = startObj.get("result").asJsonObject.get("pid").asLong
        assertTrue(pid > 0)
        
        // 2. Read output immediately (might be empty but should not fail)
        val readArgs = JsonObject()
        readArgs.addProperty("pid", pid)
        val readJsonStr = TerminalHandlers.handleReadProcessOutput(project, readArgs)
        val readObj = com.google.gson.JsonParser.parseString(readJsonStr).asJsonObject
        assertTrue(readObj.get("success").asBoolean)
        
        // 3. Kill process
        val killArgs = JsonObject()
        killArgs.addProperty("pid", pid)
        val killJsonStr = TerminalHandlers.handleKillProcess(project, killArgs)
        val killObj = com.google.gson.JsonParser.parseString(killJsonStr).asJsonObject
        assertTrue(killObj.get("success").asBoolean)
    }

    fun testHandleListRunConfigs() {
        val args = JsonObject()
        val resultJsonStr = TerminalHandlers.handleListRunConfigs(project, args)
        val resultObj = com.google.gson.JsonParser.parseString(resultJsonStr).asJsonObject
        
        // Since we are in a light test environment, there might not be any run configurations,
        // but it shouldn't crash and should return a valid JSON structure.
        assertTrue(resultObj.get("success").asBoolean)
        val configs = resultObj.get("result").asJsonObject.get("configs").asJsonArray
        assertNotNull(configs)
    }
}
}
