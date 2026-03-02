package com.forge.plugin.api.handlers

import com.google.gson.JsonObject
import com.google.gson.JsonArray
import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.execution.process.OSProcessHandler
import com.intellij.execution.process.ProcessAdapter
import com.intellij.execution.process.ProcessEvent
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.Key
import com.forge.plugin.api.ToolResult
import java.nio.charset.StandardCharsets
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicLong

object TerminalHandlers {
    private val backgroundProcesses = ConcurrentHashMap<Long, OSProcessHandler>()
    private val processOutputs = ConcurrentHashMap<Long, StringBuilder>()
    private val nextPid = AtomicLong(1000)

    fun handleExecuteCommand(project: Project, args: JsonObject): String {
        val command = args.get("command")?.asString ?: return ToolResult.error("Missing 'command' argument")
        val workDir = args.get("work_dir")?.asString ?: project.basePath

        return try {
            val commandLine = GeneralCommandLine()
            if (System.getProperty("os.name").lowercase().contains("win")) {
                commandLine.withExePath("cmd.exe").addParameters("/c", command)
            } else {
                commandLine.withExePath("/bin/sh").addParameters("-c", command)
            }
            
            commandLine.withWorkDirectory(workDir)
            commandLine.withCharset(StandardCharsets.UTF_8)

            val handler = OSProcessHandler(commandLine)
            val output = StringBuilder()
            val error = StringBuilder()

            handler.addProcessListener(object : ProcessAdapter() {
                override fun onTextAvailable(event: ProcessEvent, outputType: Key<*>) {
                    if (outputType == com.intellij.execution.process.ProcessOutputTypes.STDOUT) {
                        output.append(event.text)
                    } else if (outputType == com.intellij.execution.process.ProcessOutputTypes.STDERR) {
                        error.append(event.text)
                    }
                }
            })

            handler.startNotify()
            if (handler.waitFor(30000)) { // 30 second timeout for direct commands
                val exitCode = handler.exitCode
                val result = JsonObject()
                result.addProperty("stdout", output.toString())
                result.addProperty("stderr", error.toString())
                result.addProperty("exit_code", exitCode)
                ToolResult.success(result)
            } else {
                handler.destroyProcess()
                ToolResult.error("Command timed out after 30 seconds")
            }
        } catch (e: Exception) {
            ToolResult.error("Execution failed: ${e.message}")
        }
    }

    fun handleExecuteBackground(project: Project, args: JsonObject): String {
        val command = args.get("command")?.asString ?: return ToolResult.error("Missing 'command' argument")
        val label = args.get("label")?.asString ?: "Background Process"
        val workDir = args.get("work_dir")?.asString ?: project.basePath

        return try {
            val commandLine = GeneralCommandLine()
            if (System.getProperty("os.name").lowercase().contains("win")) {
                commandLine.withExePath("cmd.exe").addParameters("/c", command)
            } else {
                commandLine.withExePath("/bin/sh").addParameters("-c", command)
            }
            
            commandLine.withWorkDirectory(workDir)
            commandLine.withParentEnvironmentType(GeneralCommandLine.ParentEnvironmentType.CONSOLE)

            val handler = OSProcessHandler(commandLine)
            val output = StringBuilder()
            val pid = nextPid.incrementAndGet()
            processOutputs[pid] = output

            handler.addProcessListener(object : ProcessAdapter() {
                override fun onTextAvailable(event: ProcessEvent, outputType: Key<*>) {
                    if (outputType == com.intellij.execution.process.ProcessOutputTypes.STDOUT ||
                        outputType == com.intellij.execution.process.ProcessOutputTypes.STDERR) {
                        output.append(event.text)
                    }
                }
                
                override fun processTerminated(event: ProcessEvent) {
                    // Keep output but mark process as finished
                }
            })
            
            handler.startNotify()
            
            backgroundProcesses[pid] = handler
            
            val result = JsonObject()
            result.addProperty("pid", pid)
            result.addProperty("label", label)
            ToolResult.success(result)
        } catch (e: Exception) {
            ToolResult.error("Failed to start background process: ${e.message}")
        }
    }

    fun handleReadProcessOutput(project: Project, args: JsonObject): String {
        val pid = args.get("pid")?.asLong ?: return ToolResult.error("Missing 'pid' argument")
        val output = processOutputs[pid] ?: return ToolResult.error("No output found for process $pid")
        
        val tailLines = args.get("tail_lines")?.asInt ?: 100
        val lines = output.toString().lines()
        val tail = if (lines.size > tailLines) lines.takeLast(tailLines).joinToString("\n") else output.toString()
        
        val handler = backgroundProcesses[pid]
        val isRunning = handler != null && !handler.isProcessTerminated
        
        val result = JsonObject()
        result.addProperty("output", tail)
        result.addProperty("is_running", isRunning)
        return ToolResult.success(result)
    }

    fun handleKillProcess(project: Project, args: JsonObject): String {
        val pid = args.get("pid")?.asLong ?: return ToolResult.error("Missing 'pid' argument")
        val handler = backgroundProcesses.remove(pid)
        
        return if (handler != null) {
            handler.destroyProcess()
            ToolResult.success("Process $pid killed")
        } else {
            ToolResult.error("Process $pid not found in background processes")
        }
    }

    fun handleStopProject(project: Project, args: JsonObject): String {
        // Alias for kill_process if pid is provided, otherwise stop latest
        if (args.has("pid")) return handleKillProcess(project, args)
        
        val latestPid = backgroundProcesses.keys().toList().maxOrNull() ?: return ToolResult.error("No running projects found")
        return handleKillProcess(project, JsonObject().apply { addProperty("pid", latestPid) })
    }

    fun handleListRunConfigs(project: Project, args: JsonObject): String {
        return try {
            val runManager = com.intellij.execution.RunManager.getInstance(project)
            val configs = runManager.allSettings
            val results = JsonArray()
            
            for (setting in configs) {
                val configObj = JsonObject()
                configObj.addProperty("name", setting.name)
                configObj.addProperty("type", setting.type.displayName)
                results.add(configObj)
            }
            
            val result = JsonObject()
            result.add("configs", results)
            ToolResult.success(result)
        } catch (e: Exception) {
            ToolResult.error("Failed to list run configs: ${e.message}")
        }
    }

    fun handleRunProject(project: Project, args: JsonObject): String {
        val configName = args.get("config_name")?.asString
        val command = args.get("command")?.asString
        
        if (configName == null && command == null) {
            return ToolResult.error("Missing 'config_name' or 'command' argument")
        }
        
        if (command != null) {
            // If it's a raw command, run it as a background process
            return handleExecuteBackground(project, args)
        }
        
        return try {
            val runManager = com.intellij.execution.RunManager.getInstance(project)
            val setting = runManager.allSettings.find { it.name == configName }
                ?: return ToolResult.error("Run configuration '$configName' not found")
            
            com.intellij.execution.ProgramRunnerUtil.executeConfiguration(setting, com.intellij.execution.executors.DefaultRunExecutor.getRunExecutorInstance())
            ToolResult.success("Started run configuration '$configName'")
        } catch (e: Exception) {
            ToolResult.error("Failed to run project: ${e.message}")
        }
    }

    fun handleCheckProcessStatus(project: Project, args: JsonObject): String {
        val pid = args.get("pid")?.asLong ?: return ToolResult.error("Missing 'pid' argument")
        val handler = backgroundProcesses[pid]
        val isRunning = handler != null && !handler.isProcessTerminated
        
        val result = JsonObject()
        result.addProperty("pid", pid)
        result.addProperty("is_running", isRunning)
        if (handler != null) {
            result.addProperty("exit_code", if (handler.isProcessTerminated) handler.exitCode else null)
        }
        return ToolResult.success(result)
    }

    fun handleCheckPort(project: Project, args: JsonObject): String {
        val port = args.get("port")?.asInt ?: return ToolResult.error("Missing 'port' argument")
        return try {
            java.net.ServerSocket(port).use {
                ToolResult.success(mapOf("port" to port, "status" to "available"))
            }
        } catch (e: Exception) {
            ToolResult.success(mapOf("port" to port, "status" to "in_use"))
        }
    }

    fun handleWaitForPort(project: Project, args: JsonObject): String {
        val port = args.get("port")?.asInt ?: return ToolResult.error("Missing 'port' argument")
        val timeout = args.get("timeout")?.asLong ?: 30
        val start = System.currentTimeMillis()
        
        while (System.currentTimeMillis() - start < timeout * 1000) {
            try {
                java.net.Socket("localhost", port).use { return ToolResult.success("Port $port is ready") }
            } catch (e: Exception) {
                Thread.sleep(500)
            }
        }
        return ToolResult.error("Timeout waiting for port $port")
    }

    fun handleKillPort(project: Project, args: JsonObject): String {
        val port = args.get("port")?.asInt ?: return ToolResult.error("Missing 'port' argument")
        val cmd = if (System.getProperty("os.name").lowercase().contains("win")) {
            "for /f \"tokens=5\" %a in ('netstat -aon ^| findstr :$port') do taskkill /f /pid %a"
        } else {
            "lsof -ti :$port | xargs kill -9"
        }
        return handleExecuteCommand(project, JsonObject().apply { addProperty("command", cmd) })
    }

    fun handleGrep(project: Project, args: JsonObject): String {
        // Note: grep is already properly implemented in FileHandlers using IntelliJ APIs.
        // Delegate to FileHandlers for consistency.
        return FileHandlers.handleGrep(project, args)
    }

    fun handleFetchWebpage(project: Project, args: JsonObject): String {
        val url = args.get("url")?.asString ?: return ToolResult.error("Missing 'url' argument")
        
        return try {
            val content = com.intellij.util.io.HttpRequests.request(url)
                .connectTimeout(5000)
                .readTimeout(5000)
                .readString()
            
            val result = JsonObject()
            result.addProperty("content", content)
            ToolResult.success(result)
        } catch (e: Exception) {
            ToolResult.error("Failed to fetch webpage: ${e.message}")
        }
    }

    fun handleSdkManager(project: Project, args: JsonObject): String {
        // Note: This tool intentionally uses shell commands as IntelliJ has no direct API
        // for managing external SDK tools like proto, asdf, etc.
        val operation = args.get("operation")?.asString ?: return ToolResult.error("Missing 'operation' argument")
        val tool = args.get("tool")?.asString ?: ""
        val version = args.get("version")?.asString ?: "latest"
        
        return try {
            val cmd = when (operation) {
                "install" -> "proto install $tool $version"
                "list_installed" -> "proto list"
                "list_available" -> "proto list-remote $tool"
                "detect_project" -> "proto use"
                "uninstall" -> "proto uninstall $tool $version"
                "versions" -> "proto list-remote $tool"
                else -> return ToolResult.error("Unsupported SDK operation: $operation")
            }
            
            handleExecuteCommand(project, JsonObject().apply { addProperty("command", cmd) })
        } catch (e: Exception) {
            ToolResult.error("SDK operation failed: ${e.message}")
        }
    }
}
