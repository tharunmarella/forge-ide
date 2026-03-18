package com.forge.plugin.api

import com.google.gson.JsonObject
import com.intellij.openapi.project.Project
import com.forge.plugin.api.handlers.*

object ToolExecutor {
    fun execute(project: Project, toolName: String, args: JsonObject): String {
        return when (toolName) {
            // File tools
            "read_file" -> FileHandlers.handleReadFile(project, args)
            "write_file" -> FileHandlers.handleWriteToFile(project, args)
            "edit_file" -> FileHandlers.handleReplaceInFile(project, args)
            "list_files" -> FileHandlers.handleListFiles(project, args)
            "create_directory" -> FileHandlers.handleCreateDirectory(project, args)
            "duplicate_path" -> FileHandlers.handleDuplicatePath(project, args)
            "rename_path" -> FileHandlers.handleRenamePath(project, args)
            "trash_path" -> FileHandlers.handleTrashPath(project, args)
            "glob" -> FileHandlers.handleGlob(project, args)
            "delete_file" -> FileHandlers.handleDeleteFile(project, args)
            "grep" -> FileHandlers.handleGrep(project, args)
            "apply_patch" -> FileHandlers.handleApplyPatch(project, args)

            // LSP/Code tools
            "symbol_def" -> LspHandlers.handleGetSymbolDefinition(project, args)
            "analyze" -> LspHandlers.handleAnalyzeSymbolFlow(project, args)
            "lsp" -> {
                val action = args.get("action")?.asString ?: return ToolResult.error("Missing 'action' parameter for lsp tool")
                when (action) {
                    "definition" -> LspHandlers.handleLspGoToDefinition(project, args)
                    "references" -> LspHandlers.handleLspFindReferences(project, args)
                    "hover" -> LspHandlers.handleLspHover(project, args)
                    "rename" -> LspHandlers.handleLspRename(project, args)
                    "workspace_symbols" -> LspHandlers.handleGetWorkspaceSymbols(project, args)
                    "diagnostics" -> LspHandlers.handleGetDiagnostics(project, args)
                    else -> ToolResult.error("Unknown lsp action: $action")
                }
            }
            "references" -> LspHandlers.handleLspFindReferences(project, args)
            "document_symbols" -> LspHandlers.handleGetDocumentSymbols(project, args)
            "workspace_symbols" -> LspHandlers.handleGetWorkspaceSymbols(project, args)
            "diagnostics" -> ProjectHandlers.handleDiagnostics(project, args)

            // Project tools
            "map" -> ProjectHandlers.handleProjectSkeleton(project, args)
            "explore" -> ProjectHandlers.handleProjectSkeleton(project, args) // explore is often a more detailed map
            "search" -> ProjectHandlers.handleCodebaseSearch(project, args)
            "git" -> ProjectHandlers.handleGit(project, args)

            // Display tools
            "show_diagram" -> handleShowDiagram(project, args)
            "show_code" -> handleShowCode(project, args)

            // Terminal tools
            "run" -> {
                val background = args.get("background")?.asBoolean ?: false
                if (background) TerminalHandlers.handleExecuteBackground(project, args)
                else TerminalHandlers.handleExecuteCommand(project, args)
            }
            "process" -> {
                val action = args.get("action")?.asString ?: return ToolResult.error("Missing 'action' parameter for process tool")
                when (action) {
                    "output" -> TerminalHandlers.handleReadProcessOutput(project, args)
                    "status" -> TerminalHandlers.handleCheckProcessStatus(project, args)
                    "kill" -> TerminalHandlers.handleKillProcess(project, args)
                    else -> ToolResult.error("Unknown process action: $action")
                }
            }
            "port" -> {
                val action = args.get("action")?.asString ?: "check"
                when (action) {
                    "check" -> TerminalHandlers.handleCheckPort(project, args)
                    "wait" -> TerminalHandlers.handleWaitForPort(project, args)
                    "kill" -> TerminalHandlers.handleKillPort(project, args)
                    else -> ToolResult.error("Unknown port action: $action")
                }
            }
            "fetch" -> TerminalHandlers.handleFetchWebpage(project, args)
            "sdk_manager" -> TerminalHandlers.handleSdkManager(project, args)
            "run_project" -> TerminalHandlers.handleRunProject(project, args)
            "stop_project" -> TerminalHandlers.handleStopProject(project, args)
            "list_run_configs" -> TerminalHandlers.handleListRunConfigs(project, args)
            "workspace_symbols" -> ProjectHandlers.handleWorkspaceSymbols(project, args)

            // Legacy aliases
            "execute_command" -> TerminalHandlers.handleExecuteCommand(project, args)
            "execute_background" -> TerminalHandlers.handleExecuteBackground(project, args)
            "read_process_output" -> TerminalHandlers.handleReadProcessOutput(project, args)
            "kill_process" -> TerminalHandlers.handleKillProcess(project, args)
            "check_process_status" -> TerminalHandlers.handleCheckProcessStatus(project, args)
            "check_port" -> TerminalHandlers.handleCheckPort(project, args)
            "wait_for_port" -> TerminalHandlers.handleWaitForPort(project, args)
            "kill_port" -> TerminalHandlers.handleKillPort(project, args)
            "fetch_webpage" -> TerminalHandlers.handleFetchWebpage(project, args)
            "get_symbol_definition" -> LspHandlers.handleGetSymbolDefinition(project, args)
            "find_symbol_references" -> LspHandlers.handleLspFindReferences(project, args)
            "lsp_go_to_definition" -> LspHandlers.handleLspGoToDefinition(project, args)
            "lsp_find_references" -> LspHandlers.handleLspFindReferences(project, args)
            "lsp_hover" -> LspHandlers.handleLspHover(project, args)
            "lsp_rename" -> LspHandlers.handleLspRename(project, args)
            "list_code_definition_names" -> LspHandlers.handleGetDocumentSymbols(project, args)
            "write_to_file" -> FileHandlers.handleWriteToFile(project, args)
            "replace_in_file" -> FileHandlers.handleReplaceInFile(project, args)

            else -> ToolResult.error("Unknown tool: $toolName")
        }
    }
    
    private fun handleShowDiagram(project: Project, args: JsonObject): String {
        val diagramCode = args.get("diagram_code")?.asString 
            ?: return ToolResult.error("Missing 'diagram_code' parameter")
        val title = args.get("title")?.asString
        
        // Send to UI for rendering
        val uiService = project.getService(com.forge.plugin.ui.ForgeUIService::class.java)
        uiService.showDiagram(diagramCode, title)
        
        return ToolResult.success("Diagram displayed to user")
    }
    
    private fun handleShowCode(project: Project, args: JsonObject): String {
        val code = args.get("code")?.asString 
            ?: return ToolResult.error("Missing 'code' parameter")
        val language = args.get("language")?.asString ?: "text"
        
        // Send to UI for rendering
        val uiService = project.getService(com.forge.plugin.ui.ForgeUIService::class.java)
        uiService.showCode(code, language)
        
        return ToolResult.success("Code displayed to user")
    }
}
