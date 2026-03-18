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
        val timeoutMs = (args.get("timeout_secs")?.asLong ?: 120L) * 1000L

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
            if (handler.waitFor(timeoutMs)) {
                val exitCode = handler.exitCode
                val result = JsonObject()
                result.addProperty("stdout", output.toString())
                result.addProperty("stderr", error.toString())
                result.addProperty("exit_code", exitCode)
                ToolResult.success(result)
            } else {
                handler.destroyProcess()
                ToolResult.error("Command timed out after ${timeoutMs / 1000}s")
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
        
        val tailLines = args.get("tail_lines")?.asInt ?: args.get("lines")?.asInt ?: 100
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
        val background = args.get("background")?.asBoolean ?: false
        
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
            
            if (background) {
                val executor = com.intellij.execution.executors.DefaultRunExecutor.getRunExecutorInstance()
                val runner = com.intellij.execution.runners.ProgramRunner.getRunner(executor.id, setting.configuration)
                    ?: return ToolResult.error("No runner found for configuration '$configName'")
                
                val environment = com.intellij.execution.runners.ExecutionEnvironmentBuilder(project, executor)
                    .runProfile(setting.configuration)
                    .build()
                
                val pid = nextPid.incrementAndGet()
                val output = StringBuilder()
                processOutputs[pid] = output
                
                runner.execute(environment) { descriptor ->
                    val handler = descriptor.processHandler
                    if (handler != null) {
                        backgroundProcesses[pid] = handler as OSProcessHandler
                        handler.addProcessListener(object : ProcessAdapter() {
                            override fun onTextAvailable(event: ProcessEvent, outputType: Key<*>) {
                                output.append(event.text)
                            }
                        })
                    }
                }
                
                val result = JsonObject()
                result.addProperty("pid", pid)
                result.addProperty("config_name", configName)
                ToolResult.success(result)
            } else {
                com.intellij.execution.ProgramRunnerUtil.executeConfiguration(setting, com.intellij.execution.executors.DefaultRunExecutor.getRunExecutorInstance())
                ToolResult.success("Started run configuration '$configName'")
            }
        } catch (e: Exception) {
            ToolResult.error("Failed to run project: ${e.message}")
        }
    }

    fun handleCheckProcessStatus(project: Project, args: JsonObject): String {
        val pid = args.get("pid")?.asLong
        val result = JsonObject()

        if (pid == null) {
            // List all background processes
            val arr = com.google.gson.JsonArray()
            backgroundProcesses.forEach { (p, h) ->
                val obj = JsonObject()
                obj.addProperty("pid", p)
                obj.addProperty("is_running", !h.isProcessTerminated)
                arr.add(obj)
            }
            result.add("processes", arr)
            return ToolResult.success(result)
        }

        val handler = backgroundProcesses[pid]
        val isRunning = handler != null && !handler.isProcessTerminated
        result.addProperty("pid", pid)
        result.addProperty("is_running", isRunning)
        if (handler != null && handler.isProcessTerminated) {
            result.addProperty("exit_code", handler.exitCode)
        }
        return ToolResult.success(result)
    }

    fun handleGrep(project: Project, args: JsonObject): String {
        // Note: grep is already properly implemented in FileHandlers using IntelliJ APIs.
        // Delegate to FileHandlers for consistency.
        return FileHandlers.handleGrep(project, args)
    }

    fun handleCheckPort(project: Project, args: JsonObject): String {
        val port = args.get("port")?.asInt ?: args.get("port_num")?.asInt
            ?: return ToolResult.error("Missing 'port' argument")
        return try {
            java.net.ServerSocket(port).use {
                ToolResult.success(mapOf("port" to port, "status" to "available"))
            }
        } catch (e: Exception) {
            ToolResult.success(mapOf("port" to port, "status" to "in_use"))
        }
    }

    fun handleWaitForPort(project: Project, args: JsonObject): String {
        val port = args.get("port")?.asInt ?: args.get("port_num")?.asInt
            ?: return ToolResult.error("Missing 'port' argument")
        val host = args.get("host")?.asString ?: "localhost"
        val timeoutSecs = args.get("timeout")?.asLong ?: 30L
        val deadline = System.currentTimeMillis() + timeoutSecs * 1000

        // Run on a background thread to avoid blocking the EDT
        val latch = java.util.concurrent.CountDownLatch(1)
        var success = false
        Thread {
            while (System.currentTimeMillis() < deadline) {
                try {
                    java.net.Socket(host, port).use { success = true; latch.countDown(); return@Thread }
                } catch (_: Exception) {}
                Thread.sleep(500)
            }
            latch.countDown()
        }.also { it.isDaemon = true }.start()
        latch.await(timeoutSecs + 2, java.util.concurrent.TimeUnit.SECONDS)

        return if (success) ToolResult.success("Port $port is ready")
        else ToolResult.error("Timeout waiting for port $port after ${timeoutSecs}s")
    }

    fun handleKillPort(project: Project, args: JsonObject): String {
        val port = args.get("port")?.asInt ?: args.get("port_num")?.asInt
            ?: return ToolResult.error("Missing 'port' argument")
        val cmd = if (System.getProperty("os.name").lowercase().contains("win")) {
            "for /f \"tokens=5\" %a in ('netstat -aon ^| findstr :$port') do taskkill /f /pid %a"
        } else {
            "lsof -ti :$port | xargs kill -9"
        }
        return handleExecuteCommand(project, JsonObject().apply { addProperty("command", cmd) })
    }

    fun handleFetchWebpage(project: Project, args: JsonObject): String {
        val urlOrQuery = args.get("url_or_query")?.asString ?: return ToolResult.error("Missing 'url_or_query' argument")
        
        return try {
            val content = com.intellij.util.io.HttpRequests.request(urlOrQuery)
                .userAgent("Forge-IDE/1.0")
                .connectTimeout(10000)
                .readTimeout(10000)
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
        val pin = args.get("pin")?.asBoolean ?: true
        
        return try {
            val cmd = when (operation) {
                "install" -> {
                    if (tool.isEmpty()) return ToolResult.error("Missing 'tool' parameter for install operation")
                    if (version != "latest" && version.isNotEmpty()) {
                        "proto install $tool $version ${if (pin) "--pin" else ""}"
                    } else {
                        "proto install $tool ${if (pin) "--pin" else ""}"
                    }
                }
                "list_installed" -> "proto plugin list --versions"
                "list_available" -> if (tool.isNotEmpty()) "proto plugin list $tool" else "proto plugin list"
                "detect_project" -> "proto use"
                "uninstall" -> {
                    if (tool.isEmpty()) return ToolResult.error("Missing 'tool' parameter for uninstall operation")
                    if (version.isEmpty()) return ToolResult.error("Missing 'version' parameter for uninstall operation")
                    "proto uninstall $tool $version"
                }
                "versions" -> {
                    if (tool.isEmpty()) return ToolResult.error("Missing 'tool' parameter for versions operation")
                    "proto versions $tool"
                }
                else -> return ToolResult.error("Unsupported SDK operation: $operation. Valid operations: install, list_installed, list_available, detect_project, uninstall, versions")
            }
            
            handleExecuteCommand(project, JsonObject().apply { addProperty("command", cmd) })
        } catch (e: Exception) {
            ToolResult.error("SDK operation failed: ${e.message}")
        }
    }
}
