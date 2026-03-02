package com.forge.plugin.api.handlers

import com.google.gson.JsonArray
import com.google.gson.JsonObject
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.roots.ProjectFileIndex
import com.intellij.codeInsight.daemon.impl.DaemonCodeAnalyzerEx
import com.intellij.codeInsight.daemon.impl.HighlightInfo
import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.vfs.LocalFileSystem
import com.forge.plugin.api.ToolResult

object ProjectHandlers {

    fun handleDiagnostics(project: Project, args: JsonObject): String {
        val path = args.get("path")?.asString
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        
        val resultsArray = JsonArray()
        
        ApplicationManager.getApplication().runReadAction {
            if (path != null) {
                val virtualFile = LocalFileSystem.getInstance().findFileByPath("$basePath/$path")
                if (virtualFile != null) {
                    collectDiagnosticsForFile(project, virtualFile, resultsArray)
                }
            } else {
                ProjectFileIndex.getInstance(project).iterateContent { file ->
                    if (!file.isDirectory) {
                        collectDiagnosticsForFile(project, file, resultsArray)
                    }
                    true
                }
            }
        }
        
        val finalObj = JsonObject()
        finalObj.add("diagnostics", resultsArray)
        return ToolResult.success(finalObj)
    }
    
    private fun collectDiagnosticsForFile(
        project: Project,
        virtualFile: com.intellij.openapi.vfs.VirtualFile,
        resultsArray: JsonArray
    ) {
        try {
            val document = FileDocumentManager.getInstance().getDocument(virtualFile) ?: return
            val psiFile = com.intellij.psi.PsiManager.getInstance(project).findFile(virtualFile) ?: return
            
            val analysisResult = com.intellij.codeInsight.daemon.impl.DaemonCodeAnalyzerImpl.getHighlights(
                document, HighlightSeverity.ERROR, project
            )
            
            for (info in analysisResult) {
                val obj = JsonObject()
                obj.addProperty("path", virtualFile.path.removePrefix("${project.basePath ?: ""}/"))
                obj.addProperty("line", document.getLineNumber(info.startOffset))
                obj.addProperty("message", info.description)
                obj.addProperty("severity", "error")
                resultsArray.add(obj)
            }
        } catch (e: Exception) {
            // Skip files that can't be analyzed
        }
    }

    fun handleProjectSkeleton(project: Project, args: JsonObject): String {
        val view = args.get("view")?.asString ?: "map"
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        
        val result = JsonObject()
        result.addProperty("project_name", project.name)
        result.addProperty("base_path", basePath)
        
        val structure = JsonArray()
        
        ApplicationManager.getApplication().runReadAction {
            // Get all source roots
            val projectRootManager = com.intellij.openapi.roots.ProjectRootManager.getInstance(project)
            val contentRoots = projectRootManager.contentSourceRoots
            
            for (root in contentRoots) {
                val rootInfo = JsonObject()
                rootInfo.addProperty("path", root.path.removePrefix("$basePath/"))
                rootInfo.addProperty("type", "source_root")
                
                // Count files and directories
                var fileCount = 0
                var dirCount = 0
                com.intellij.openapi.vfs.VfsUtil.iterateChildrenRecursively(root, null) { file ->
                    if (file.isDirectory) dirCount++ else fileCount++
                    true
                }
                rootInfo.addProperty("file_count", fileCount)
                rootInfo.addProperty("dir_count", dirCount)
                
                structure.add(rootInfo)
            }
            
            // Get key configuration files
            val configFiles = listOf(
                "pom.xml", "build.gradle", "build.gradle.kts", "settings.gradle", "settings.gradle.kts",
                "package.json", "Cargo.toml", "go.mod", "requirements.txt", "setup.py",
                ".gitignore", "README.md", "LICENSE"
            )
            
            val foundConfigs = JsonArray()
            for (filename in configFiles) {
                val file = com.intellij.openapi.vfs.LocalFileSystem.getInstance()
                    .findFileByPath("$basePath/$filename")
                if (file != null) {
                    foundConfigs.add(filename)
                }
            }
            result.add("config_files", foundConfigs)
        }
        
        result.add("structure", structure)
        return ToolResult.success(result)
    }

    fun handleCodebaseSearch(project: Project, args: JsonObject): String {
        // NOTE: codebase_search is a SERVER-SIDE tool handled by forge-search backend.
        // The plugin should never receive this tool call - it's executed on the server.
        // If we receive it, something is wrong with the backend configuration.
        return ToolResult.error("codebase_search is a server-side tool and should not be executed by the plugin. This indicates a backend configuration issue.")
    }

    fun handleGit(project: Project, args: JsonObject): String {
        val operation = args.get("operation").asString
        val paths = args.getAsJsonArray("paths")?.map { it.asString }
        val message = args.get("message")?.asString
        val action = args.get("action")?.asString
        val name = args.get("name")?.asString
        val limit = args.get("limit")?.asInt ?: 10
        val path = args.get("path")?.asString
        val staged = args.get("staged")?.asBoolean ?: false
        
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        
        return try {
            val gitRepository = git4idea.repo.GitRepositoryManager.getInstance(project).repositories.firstOrNull()
                ?: return ToolResult.error("No git repository found")
            
            when (operation) {
                "status" -> {
                    val changeListManager = com.intellij.openapi.vcs.changes.ChangeListManager.getInstance(project)
                    val changes = changeListManager.allChanges
                    val resultsArray = JsonArray()
                    changes.forEach { change ->
                        val obj = JsonObject()
                        obj.addProperty("path", change.virtualFile?.path?.removePrefix("$basePath/") ?: "")
                        obj.addProperty("status", change.type.toString())
                        resultsArray.add(obj)
                    }
                    val result = JsonObject()
                    result.add("changes", resultsArray)
                    ToolResult.success(result)
                }
                "log" -> {
                    val commits = mutableListOf<String>()
                    
                    ApplicationManager.getApplication().runReadAction {
                        try {
                            val handler = git4idea.commands.GitLineHandler(project, gitRepository.root, git4idea.commands.GitCommand.LOG)
                            handler.addParameters("-n", limit.toString(), "--oneline")
                            val result = git4idea.commands.Git.getInstance().runCommand(handler)
                            commits.addAll(result.output)
                        } catch (e: Exception) {
                            // Fallback handled below
                        }
                    }
                    
                    val result = JsonObject()
                    result.addProperty("log", commits.joinToString("\n"))
                    ToolResult.success(result)
                }
                "branch" -> {
                    when (action) {
                        "list" -> {
                            val branches = gitRepository.branches
                            val result = JsonObject()
                            val branchArray = JsonArray()
                            branches.localBranches.forEach { branch -> 
                                branchArray.add(branch.name)
                            }
                            result.add("branches", branchArray)
                            result.addProperty("current", gitRepository.currentBranch?.name ?: "")
                            ToolResult.success(result)
                        }
                        else -> ToolResult.error("Git branch operation '$action' not fully implemented via IntelliJ API")
                    }
                }
                else -> {
                    ToolResult.error("Git operation '$operation' not fully implemented via IntelliJ API. Requires further work.")
                }
            }
        } catch (e: Exception) {
            ToolResult.error("Git operation failed: ${e.message}")
        }
    }

    fun handleWorkspaceSymbols(project: Project, args: JsonObject): String {
        val query = args.get("query")?.asString ?: ""
        val results = JsonArray()
        
        ApplicationManager.getApplication().runReadAction {
            val scope = com.intellij.psi.search.GlobalSearchScope.projectScope(project)
            val cache = com.intellij.psi.search.PsiShortNamesCache.getInstance(project)
            
            try {
                // Search for methods
                cache.getMethodsByName(query, scope).forEach { method ->
                    val obj = JsonObject()
                    obj.addProperty("name", method.name)
                    obj.addProperty("kind", "method")
                    obj.addProperty("containerName", method.containingClass?.qualifiedName)
                    obj.addProperty("path", method.containingFile.virtualFile.path.removePrefix("${project.basePath ?: ""}/"))
                    results.add(obj)
                }
                
                // Search for classes
                cache.getClassesByName(query, scope).forEach { clazz ->
                    val obj = JsonObject()
                    obj.addProperty("name", clazz.name)
                    obj.addProperty("kind", "class")
                    obj.addProperty("containerName", clazz.qualifiedName)
                    obj.addProperty("path", clazz.containingFile.virtualFile.path.removePrefix("${project.basePath ?: ""}/"))
                    results.add(obj)
                }
                
                // Search for fields
                cache.getFieldsByName(query, scope).forEach { field ->
                    val obj = JsonObject()
                    obj.addProperty("name", field.name)
                    obj.addProperty("kind", "field")
                    obj.addProperty("containerName", field.containingClass?.qualifiedName)
                    obj.addProperty("path", field.containingFile.virtualFile.path.removePrefix("${project.basePath ?: ""}/"))
                    results.add(obj)
                }
            } catch (e: Exception) {
                return@runReadAction
            }
        }
        
        return ToolResult.success(results)
    }
}