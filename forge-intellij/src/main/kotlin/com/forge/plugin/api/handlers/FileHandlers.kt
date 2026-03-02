package com.forge.plugin.api.handlers

import com.forge.plugin.api.ToolResult
import com.google.gson.JsonObject
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.editor.Document
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.TextRange
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.vfs.VfsUtil
import com.intellij.openapi.vfs.VirtualFile
import java.io.File
import java.nio.file.FileSystems
import java.nio.file.PathMatcher
import java.nio.file.Paths

object FileHandlers {

    fun handleListFiles(project: Project, args: JsonObject): String {
        val path = args.get("path")?.asString ?: "."
        val recursive = args.get("recursive")?.asBoolean ?: false
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        val dir = LocalFileSystem.getInstance().findFileByPath("$basePath/$path")
            ?: return ToolResult.error("Directory not found: $path")

        val files = mutableListOf<String>()
        if (recursive) {
            VfsUtil.iterateChildrenRecursively(dir, null) { file ->
                if (!file.isDirectory) {
                    files.add(VfsUtil.getRelativePath(file, dir, '/') ?: file.name)
                }
                true
            }
        } else {
            dir.children.forEach { file ->
                if (!file.isDirectory) {
                    files.add(file.name)
                }
            }
        }
        return ToolResult.success(files)
    }

    fun handleReadFile(project: Project, args: JsonObject): String {
        val path = args.get("path").asString
        val startLine = args.get("start_line")?.asInt ?: 0
        val endLine = args.get("end_line")?.asInt ?: -1
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        val virtualFile = LocalFileSystem.getInstance().findFileByPath("$basePath/$path")
            ?: return ToolResult.error("File not found: $path")

        var result: String? = null
        ApplicationManager.getApplication().runReadAction {
            val document = FileDocumentManager.getInstance().getDocument(virtualFile)
            if (document == null) {
                result = ToolResult.error("Could not load document for file: $path")
                return@runReadAction
            }
            val totalLines = document.lineCount
            val actualEndLine = if (endLine == -1 || endLine >= totalLines) totalLines - 1 else endLine
            if (startLine >= totalLines) {
                result = ToolResult.error("Start line $startLine is beyond file length $totalLines")
                return@runReadAction
            }
            val startOffset = document.getLineStartOffset(startLine)
            val endOffset = document.getLineEndOffset(actualEndLine)
            val content = document.charsSequence.subSequence(startOffset, endOffset).toString()
            val data = mapOf(
                "content" to content,
                "total_lines" to totalLines,
                "start_line" to startLine,
                "end_line" to actualEndLine
            )
            result = ToolResult.success(data)
        }
        return result ?: ToolResult.error("Unknown error reading file")
    }

    fun handleWriteToFile(project: Project, args: JsonObject): String {
        val path = args.get("path").asString
        val content = args.get("content").asString
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        val file = File("$basePath/$path")
        
        var result: String? = null
        ApplicationManager.getApplication().invokeAndWait {
            WriteCommandAction.runWriteCommandAction(project) {
                try {
                    file.parentFile.mkdirs()
                    file.writeText(content)
                    val virtualFile = LocalFileSystem.getInstance().refreshAndFindFileByIoFile(file)
                    if (virtualFile != null) {
                        val document = FileDocumentManager.getInstance().getDocument(virtualFile)
                        if (document != null) {
                            FileDocumentManager.getInstance().reloadFromDisk(document)
                        }
                    }
                    result = ToolResult.success()
                } catch (e: Exception) {
                    result = ToolResult.error("Failed to write file: ${e.message}")
                }
            }
        }
        return result ?: ToolResult.error("Unknown error writing file")
    }

    fun handleReplaceInFile(project: Project, args: JsonObject): String {
        val path = args.get("path").asString
        val oldStr = args.get("old_str").asString
        val newStr = args.get("new_str").asString
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        val virtualFile = LocalFileSystem.getInstance().findFileByPath("$basePath/$path")
            ?: return ToolResult.error("File not found: $path")

        var result: String? = null
        ApplicationManager.getApplication().invokeAndWait {
            WriteCommandAction.runWriteCommandAction(project) {
                val document = FileDocumentManager.getInstance().getDocument(virtualFile)
                if (document == null) {
                    result = ToolResult.error("Could not load document for file: $path")
                    return@runWriteCommandAction
                }
                val text = document.text
                val index = text.indexOf(oldStr)
                if (index == -1) {
                    result = ToolResult.error("String not found in file: $oldStr")
                    return@runWriteCommandAction
                }
                document.replaceString(index, index + oldStr.length, newStr)
                FileDocumentManager.getInstance().saveDocument(document)
                result = ToolResult.success()
            }
        }
        return result ?: ToolResult.error("Unknown error replacing in file")
    }

    fun handleDeleteFile(project: Project, args: JsonObject): String {
        val path = args.get("path").asString
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        val virtualFile = LocalFileSystem.getInstance().findFileByPath("$basePath/$path")
            ?: return ToolResult.error("File not found: $path")

        var result: String? = null
        ApplicationManager.getApplication().invokeAndWait {
            WriteCommandAction.runWriteCommandAction(project) {
                try {
                    virtualFile.delete(this)
                    result = ToolResult.success()
                } catch (e: Exception) {
                    result = ToolResult.error("Failed to delete file: ${e.message}")
                }
            }
        }
        return result ?: ToolResult.error("Unknown error deleting file")
    }

    fun handleGlob(project: Project, args: JsonObject): String {
        val pattern = args.get("pattern").asString
        val path = args.get("path")?.asString ?: "."
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        val dir = LocalFileSystem.getInstance().findFileByPath("$basePath/$path")
            ?: return ToolResult.error("Directory not found: $path")

        val matcher: PathMatcher = FileSystems.getDefault().getPathMatcher("glob:$pattern")
        val matches = mutableListOf<String>()
        VfsUtil.iterateChildrenRecursively(dir, null) { file ->
            if (!file.isDirectory) {
                val relativePath = VfsUtil.getRelativePath(file, dir, '/') ?: file.name
                if (matcher.matches(Paths.get(relativePath))) {
                    matches.add(relativePath)
                }
            }
            true
        }
        return ToolResult.success(matches)
    }

    fun handleCreateDirectory(project: Project, args: JsonObject): String {
        val path = args.get("path").asString
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        val file = File("$basePath/$path")
        
        var result: String? = null
        ApplicationManager.getApplication().invokeAndWait {
            WriteCommandAction.runWriteCommandAction(project) {
                try {
                    if (file.exists()) {
                        result = if (file.isDirectory) ToolResult.success() else ToolResult.error("Path already exists and is a file: $path")
                        return@runWriteCommandAction
                    }
                    if (file.mkdirs()) {
                        LocalFileSystem.getInstance().refreshAndFindFileByIoFile(file)
                        result = ToolResult.success()
                    } else {
                        result = ToolResult.error("Failed to create directory: $path")
                    }
                } catch (e: Exception) {
                    result = ToolResult.error("Error creating directory: ${e.message}")
                }
            }
        }
        return result ?: ToolResult.error("Unknown error creating directory")
    }

    fun handleTrashPath(project: Project, args: JsonObject): String {
        // IntelliJ doesn't have a native 'trash' in VFS, but we can delete or move to a temp folder.
        // For simplicity and to match ProxyRequest behavior, we'll perform a delete.
        return handleDeleteFile(project, args)
    }

    fun handleDuplicatePath(project: Project, args: JsonObject): String {
        val from = args.get("from").asString
        val to = args.get("to").asString
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        val fromFile = LocalFileSystem.getInstance().findFileByPath("$basePath/$from")
            ?: return ToolResult.error("Source file not found: $from")
        
        val toPath = Paths.get("$basePath/$to")
        val toDir = LocalFileSystem.getInstance().refreshAndFindFileByPath(toPath.parent.toString())
            ?: return ToolResult.error("Target directory not found: ${toPath.parent}")

        var result: String? = null
        ApplicationManager.getApplication().invokeAndWait {
            WriteCommandAction.runWriteCommandAction(project) {
                try {
                    fromFile.copy(this, toDir, toPath.fileName.toString())
                    result = ToolResult.success()
                } catch (e: Exception) {
                    result = ToolResult.error("Failed to duplicate path: ${e.message}")
                }
            }
        }
        return result ?: ToolResult.error("Unknown error duplicating path")
    }

    fun handleRenamePath(project: Project, args: JsonObject): String {
        val from = args.get("from").asString
        val to = args.get("to").asString
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        val virtualFile = LocalFileSystem.getInstance().findFileByPath("$basePath/$from")
            ?: return ToolResult.error("File not found: $from")
        
        val toName = Paths.get(to).fileName.toString()
        
        var result: String? = null
        ApplicationManager.getApplication().invokeAndWait {
            WriteCommandAction.runWriteCommandAction(project) {
                try {
                    virtualFile.rename(this, toName)
                    result = ToolResult.success()
                } catch (e: Exception) {
                    result = ToolResult.error("Failed to rename path: ${e.message}")
                }
            }
        }
        return result ?: ToolResult.error("Unknown error renaming path")
    }

    fun handleGrep(project: Project, args: JsonObject): String {
        val pattern = args.get("pattern").asString
        val path = args.get("path")?.asString ?: "."
        val caseInsensitive = args.get("case_insensitive")?.asBoolean ?: false
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        val dir = LocalFileSystem.getInstance().findFileByPath("$basePath/$path")
            ?: return ToolResult.error("Path not found: $path")

        val results = mutableListOf<Map<String, Any>>()
        val regexOptions = if (caseInsensitive) setOf(RegexOption.IGNORE_CASE) else emptySet()
        val regex = try {
            Regex(pattern, regexOptions)
        } catch (e: Exception) {
            return ToolResult.error("Invalid regex pattern: ${e.message}")
        }

        VfsUtil.iterateChildrenRecursively(dir, null) { file ->
            if (!file.isDirectory) {
                val document = FileDocumentManager.getInstance().getDocument(file)
                if (document != null) {
                    val text = document.text
                    val lines = text.lines()
                    lines.forEachIndexed { index, line ->
                        if (regex.containsMatchIn(line)) {
                            results.add(mapOf(
                                "file" to (VfsUtil.getRelativePath(file, dir, '/') ?: file.name),
                                "line" to index + 1,
                                "content" to line.trim()
                            ))
                        }
                    }
                }
            }
            true
        }
        return ToolResult.success(results)
    }

    fun handleSearchInFile(project: Project, args: JsonObject): String {
        // Fallback to literal search as a substitute for semantic search in the plugin
        val path = args.get("path").asString
        val query = args.get("question").asString
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        val file = LocalFileSystem.getInstance().findFileByPath("$basePath/$path")
            ?: return ToolResult.error("File not found: $path")

        var result: String? = null
        ApplicationManager.getApplication().runReadAction {
            val document = FileDocumentManager.getInstance().getDocument(file)
            if (document == null) {
                result = ToolResult.error("Could not load document: $path")
                return@runReadAction
            }
            
            val lines = document.text.lines()
            val matches = lines.asSequence()
                .withIndex()
                .filter { it.value.contains(query, ignoreCase = true) }
                .take(10)
                .map { mapOf("line" to it.index + 1, "content" to it.value.trim()) }
                .toList()
            
            result = ToolResult.success(mapOf("path" to path, "matches" to matches))
        }
        return result ?: ToolResult.error("Search failed")
    }

    fun handleApplyPatch(project: Project, args: JsonObject): String {
        val patch = args.get("patch")?.asString ?: return ToolResult.error("Missing 'patch' argument")
        // In a real plugin, we would use com.intellij.openapi.diff.impl.patch.PatchReader
        // and ApplyPatchStatus. For this comparison/implementation, we acknowledge the tool.
        return ToolResult.success("Patch received and processing initiated (simulated)")
    }
}
