package com.forge.plugin.api

import com.google.gson.JsonObject
import com.intellij.openapi.project.Project
import com.forge.plugin.api.handlers.*

object ToolExecutor {
    fun execute(project: Project, toolName: String, args: JsonObject): String {
        return when (toolName) {
            // File tools
            "read_file" -> FileHandlers.handleReadFile(project, args)
            "write_to_file" -> FileHandlers.handleWriteToFile(project, args)
            "replace_in_file" -> FileHandlers.handleReplaceInFile(project, args)
            "list_files" -> FileHandlers.handleListFiles(project, args)
            "glob" -> FileHandlers.handleGlob(project, args)
            "delete_file" -> FileHandlers.handleDeleteFile(project, args)
            "search_in_file" -> FileHandlers.handleSearchInFile(project, args)
            "grep" -> FileHandlers.handleGrep(project, args)

            // LSP/Code tools
            "get_symbol_definition" -> LspHandlers.handleGetSymbolDefinition(project, args)
            "analyze_symbol_flow" -> LspHandlers.handleAnalyzeSymbolFlow(project, args)
            "lsp_go_to_definition" -> LspHandlers.handleLspGoToDefinition(project, args)
            "lsp_find_references" -> LspHandlers.handleLspFindReferences(project, args)
            "lsp_rename" -> LspHandlers.handleLspRename(project, args)
            "lsp_hover" -> LspHandlers.handleLspHover(project, args)
            "list_code_definition_names" -> LspHandlers.handleGetDocumentSymbols(project, args)
            "get_workspace_symbols" -> LspHandlers.handleGetWorkspaceSymbols(project, args)
            "get_diagnostics" -> LspHandlers.handleGetDiagnostics(project, args)

            // Project tools
            "get_architecture_map" -> ProjectHandlers.handleProjectSkeleton(project, args)
            "diagnostics" -> ProjectHandlers.handleDiagnostics(project, args)
            "git" -> ProjectHandlers.handleGit(project, args)
            "codebase_search" -> ProjectHandlers.handleCodebaseSearch(project, args)

            // Display tools (show in UI, not execute)
            "show_diagram" -> handleShowDiagram(project, args)
            "show_code" -> handleShowCode(project, args)

            // Terminal tools
            "execute_command" -> TerminalHandlers.handleExecuteCommand(project, args)
            "execute_background" -> TerminalHandlers.handleExecuteBackground(project, args)
            "read_process_output" -> TerminalHandlers.handleReadProcessOutput(project, args)
            "kill_process" -> TerminalHandlers.handleKillProcess(project, args)
            "stop_project" -> TerminalHandlers.handleStopProject(project, args)
            "list_run_configs" -> TerminalHandlers.handleListRunConfigs(project, args)
            "fetch_webpage" -> TerminalHandlers.handleFetchWebpage(project, args)

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
