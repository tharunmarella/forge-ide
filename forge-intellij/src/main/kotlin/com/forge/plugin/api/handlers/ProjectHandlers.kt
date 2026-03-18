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
        val focusPath = args.get("focus_path")?.asString ?: ""
        val view = args.get("view")?.asString ?: "map"
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        val rootDir = if (focusPath.isNotEmpty()) LocalFileSystem.getInstance().findFileByPath("$basePath/$focusPath") else project.projectFile?.parent
        
        if (rootDir == null) return ToolResult.error("Root directory not found")

        val skeleton = StringBuilder()
        buildSkeleton(project, rootDir, skeleton, 0, view == "skeleton")
        return ToolResult.success(skeleton.toString())
    }

    private val SKIP_SKELETON_DIRS = setOf(
        "node_modules", ".git", "target", "build", "dist", ".idea",
        "__pycache__", ".gradle", ".next", "out", "coverage"
    )

    private fun buildSkeleton(project: Project, dir: com.intellij.openapi.vfs.VirtualFile, sb: StringBuilder, indent: Int, showSignatures: Boolean) {
        if (dir.name in SKIP_SKELETON_DIRS) return
        val prefix = "  ".repeat(indent)
        sb.append("$prefix${dir.name}/\n")
        dir.children.sortedWith(compareBy({ !it.isDirectory }, { it.name })).forEach { file ->
            if (file.isDirectory) {
                buildSkeleton(project, file, sb, indent + 1, showSignatures)
            } else {
                sb.append("$prefix  ${file.name}\n")
                if (showSignatures && !file.fileType.isBinary) {
                    extractFileSignatures(project, file).forEach { sig ->
                        sb.append("$prefix    $sig\n")
                    }
                }
            }
        }
    }

    private fun extractFileSignatures(project: Project, vFile: com.intellij.openapi.vfs.VirtualFile): List<String> {
        val sigs = mutableListOf<String>()
        com.intellij.openapi.application.ApplicationManager.getApplication().runReadAction {
            val psiFile = com.intellij.psi.PsiManager.getInstance(project).findFile(vFile) ?: return@runReadAction
            psiFile.accept(object : com.intellij.psi.PsiRecursiveElementVisitor() {
                override fun visitElement(element: com.intellij.psi.PsiElement) {
                    if (element is com.intellij.psi.PsiNamedElement) {
                        val typeName = element.javaClass.simpleName
                        val isTopLevel = typeName.contains("Function", true) ||
                                typeName.contains("Method", true) ||
                                typeName.contains("Class", true) ||
                                typeName.contains("Interface", true) ||
                                typeName.contains("Enum", true)
                        if (isTopLevel) {
                            val name = element.name ?: return
                            // Extract first line (signature) of element text
                            val sig = element.text.lineSequence().first().trim().take(120)
                            sigs.add(sig)
                        }
                    }
                    if (sigs.size < 50) super.visitElement(element)
                }
            })
        }
        return sigs
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
            
            val git = git4idea.commands.Git.getInstance()
            
            when (operation) {
                "status" -> {
                    val handler = git4idea.commands.GitLineHandler(project, gitRepository.root, git4idea.commands.GitCommand.STATUS)
                    handler.addParameters("--porcelain")
                    val result = git.runCommand(handler)
                    if (result.success()) {
                        val obj = JsonObject()
                        obj.addProperty("status", result.outputAsJoinedString)
                        ToolResult.success(obj)
                    } else {
                        ToolResult.error(result.errorOutputAsJoinedString)
                    }
                }
                "stage" -> {
                    if (paths == null || paths.isEmpty()) return ToolResult.error("Missing 'paths' for stage operation")
                    val handler = git4idea.commands.GitLineHandler(project, gitRepository.root, git4idea.commands.GitCommand.ADD)
                    handler.addParameters(paths)
                    val result = git.runCommand(handler)
                    if (result.success()) ToolResult.success("Staged ${paths.size} files")
                    else ToolResult.error(result.errorOutputAsJoinedString)
                }
                "unstage" -> {
                    if (paths == null || paths.isEmpty()) return ToolResult.error("Missing 'paths' for unstage operation")
                    val handler = git4idea.commands.GitLineHandler(project, gitRepository.root, git4idea.commands.GitCommand.RESET)
                    handler.addParameters("HEAD")
                    handler.addParameters(paths)
                    val result = git.runCommand(handler)
                    if (result.success()) ToolResult.success("Unstaged ${paths.size} files")
                    else ToolResult.error(result.errorOutputAsJoinedString)
                }
                "commit" -> {
                    if (message == null) return ToolResult.error("Missing 'message' for commit operation")
                    val handler = git4idea.commands.GitLineHandler(project, gitRepository.root, git4idea.commands.GitCommand.COMMIT)
                    handler.addParameters("-m", message)
                    val result = git.runCommand(handler)
                    if (result.success()) ToolResult.success("Committed changes")
                    else ToolResult.error(result.errorOutputAsJoinedString)
                }
                "push" -> {
                    val handler = git4idea.commands.GitLineHandler(project, gitRepository.root, git4idea.commands.GitCommand.PUSH)
                    val result = git.runCommand(handler)
                    if (result.success()) ToolResult.success("Pushed changes")
                    else ToolResult.error(result.errorOutputAsJoinedString)
                }
                "pull" -> {
                    val handler = git4idea.commands.GitLineHandler(project, gitRepository.root, git4idea.commands.GitCommand.PULL)
                    val result = git.runCommand(handler)
                    if (result.success()) ToolResult.success("Pulled changes")
                    else ToolResult.error(result.errorOutputAsJoinedString)
                }
                "log" -> {
                    val handler = git4idea.commands.GitLineHandler(project, gitRepository.root, git4idea.commands.GitCommand.LOG)
                    handler.addParameters("-n", limit.toString(), "--oneline")
                    val result = git.runCommand(handler)
                    if (result.success()) {
                        val obj = JsonObject()
                        obj.addProperty("log", result.outputAsJoinedString)
                        ToolResult.success(obj)
                    } else {
                        ToolResult.error(result.errorOutputAsJoinedString)
                    }
                }
                "branch" -> {
                    when (action) {
                        "list" -> {
                            val handler = git4idea.commands.GitLineHandler(project, gitRepository.root, git4idea.commands.GitCommand.BRANCH)
                            val result = git.runCommand(handler)
                            if (result.success()) {
                                val obj = JsonObject()
                                obj.addProperty("branches", result.outputAsJoinedString)
                                ToolResult.success(obj)
                            } else {
                                ToolResult.error(result.errorOutputAsJoinedString)
                            }
                        }
                        "create" -> {
                            if (name == null) return ToolResult.error("Missing 'name' for branch create")
                            val handler = git4idea.commands.GitLineHandler(project, gitRepository.root, git4idea.commands.GitCommand.CHECKOUT)
                            handler.addParameters("-b", name)
                            val result = git.runCommand(handler)
                            if (result.success()) ToolResult.success("Created and switched to branch $name")
                            else ToolResult.error(result.errorOutputAsJoinedString)
                        }
                        "switch" -> {
                            if (name == null) return ToolResult.error("Missing 'name' for branch switch")
                            val handler = git4idea.commands.GitLineHandler(project, gitRepository.root, git4idea.commands.GitCommand.CHECKOUT)
                            handler.addParameters(name)
                            val result = git.runCommand(handler)
                            if (result.success()) ToolResult.success("Switched to branch $name")
                            else ToolResult.error(result.errorOutputAsJoinedString)
                        }
                        else -> ToolResult.error("Unknown branch action: $action")
                    }
                }
                "diff" -> {
                    val handler = git4idea.commands.GitLineHandler(project, gitRepository.root, git4idea.commands.GitCommand.DIFF)
                    if (staged) handler.addParameters("--cached")
                    if (path != null) handler.addParameters(path)
                    val result = git.runCommand(handler)
                    if (result.success()) {
                        val obj = JsonObject()
                        obj.addProperty("diff", result.outputAsJoinedString)
                        ToolResult.success(obj)
                    } else {
                        ToolResult.error(result.errorOutputAsJoinedString)
                    }
                }
                else -> ToolResult.error("Unknown git operation: $operation")
            }
        } catch (e: Exception) {
            ToolResult.error("Git operation failed: ${e.message}")
        }
    }

    fun handleWorkspaceSymbols(project: Project, args: JsonObject): String {
        val query = args.get("query")?.asString?.takeIf { it.isNotBlank() }
            ?: return ToolResult.error("Missing 'query' argument")
        val limit = args.get("limit")?.asInt ?: 50
        val results = JsonArray()
        val basePath = project.basePath ?: ""

        ApplicationManager.getApplication().runReadAction {
            val scope = com.intellij.psi.search.GlobalSearchScope.projectScope(project)
            val cache = com.intellij.psi.search.PsiShortNamesCache.getInstance(project)

            try {
                // Partial/fuzzy match: filter all known names that contain the query substring
                val matchingClassNames = cache.allClassNames.filter { it.contains(query, ignoreCase = true) }
                for (name in matchingClassNames) {
                    if (results.size() >= limit) break
                    cache.getClassesByName(name, scope).forEach clazz@{ clazz ->
                        if (results.size() >= limit) return@clazz
                        val file = clazz.containingFile?.virtualFile ?: return@clazz
                        val obj = JsonObject()
                        obj.addProperty("name", clazz.name)
                        obj.addProperty("kind", "class")
                        obj.addProperty("qualified_name", clazz.qualifiedName)
                        obj.addProperty("path", file.path.removePrefix("$basePath/"))
                        results.add(obj)
                    }
                }

                val matchingMethodNames = cache.allMethodNames.filter { it.contains(query, ignoreCase = true) }
                for (name in matchingMethodNames) {
                    if (results.size() >= limit) break
                    cache.getMethodsByName(name, scope).forEach method@{ method ->
                        if (results.size() >= limit) return@method
                        val file = method.containingFile?.virtualFile ?: return@method
                        val obj = JsonObject()
                        obj.addProperty("name", method.name)
                        obj.addProperty("kind", "method")
                        obj.addProperty("container", method.containingClass?.qualifiedName)
                        obj.addProperty("path", file.path.removePrefix("$basePath/"))
                        results.add(obj)
                    }
                }

                val matchingFieldNames = cache.allFieldNames.filter { it.contains(query, ignoreCase = true) }
                for (name in matchingFieldNames) {
                    if (results.size() >= limit) break
                    cache.getFieldsByName(name, scope).forEach field@{ field ->
                        if (results.size() >= limit) return@field
                        val file = field.containingFile?.virtualFile ?: return@field
                        val obj = JsonObject()
                        obj.addProperty("name", field.name)
                        obj.addProperty("kind", "field")
                        obj.addProperty("container", field.containingClass?.qualifiedName)
                        obj.addProperty("path", file.path.removePrefix("$basePath/"))
                        results.add(obj)
                    }
                }
            } catch (e: Exception) {
                return@runReadAction
            }
        }

        return ToolResult.success(results)
    }
}
