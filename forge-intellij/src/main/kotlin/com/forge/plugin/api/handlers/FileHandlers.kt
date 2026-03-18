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
import com.intellij.openapi.vfs.VfsUtilCore
import com.intellij.openapi.vfs.VirtualFile
import java.io.File
import java.nio.file.FileSystems
import java.nio.file.PathMatcher
import java.nio.file.Paths

object FileHandlers {

    fun handleListFiles(project: Project, args: JsonObject): String {
        val path = args.get("path")?.asString ?: ""
        val recursive = args.get("recursive")?.asBoolean ?: false
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        val baseDir = LocalFileSystem.getInstance().findFileByPath(basePath) ?: return ToolResult.error("Project base directory not found")
        val rootDir = if (path.isNotEmpty()) VfsUtil.findRelativeFile(path, baseDir) else baseDir
        
        if (rootDir == null) return ToolResult.error("Directory not found: $path")

        val files = mutableListOf<String>()
        VfsUtilCore.iterateChildrenRecursively(rootDir, null) { file ->
            if (!file.isDirectory) {
                files.add(VfsUtilCore.getRelativePath(file, rootDir) ?: file.name)
            }
            recursive || file == rootDir
        }
        return ToolResult.success(files.joinToString("\n"))
    }

    fun handleReadFile(project: Project, args: JsonObject): String {
        val path = args.get("path").asString
        val startLine = args.get("start_line")?.asInt ?: 0
        val endLine = args.get("end_line")?.asInt ?: -1
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        val baseDir = LocalFileSystem.getInstance().findFileByPath(basePath) ?: return ToolResult.error("Project base directory not found")
        val virtualFile = VfsUtil.findRelativeFile(path, baseDir)
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
            
            if (startLine < 0 || startLine >= totalLines) {
                result = ToolResult.error("Start line $startLine is out of bounds (0-$totalLines)")
                return@runReadAction
            }

            val startOffset = document.getLineStartOffset(startLine)
            val endOffset = document.getLineEndOffset(actualEndLine)
            val content = document.charsSequence.subSequence(startOffset, endOffset).toString()
            
            result = ToolResult.success(content)
        }
        return result ?: ToolResult.error("Unknown error reading file")
    }

    fun handleWriteToFile(project: Project, args: JsonObject): String {
        val path = args.get("path").asString
        val content = args.get("content").asString
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        val baseDir = LocalFileSystem.getInstance().findFileByPath(basePath) ?: return ToolResult.error("Project base directory not found")
        
        var result: String? = null
        ApplicationManager.getApplication().invokeAndWait {
            WriteCommandAction.runWriteCommandAction(project) {
                try {
                    val file = File(basePath, path)
                    file.parentFile.mkdirs()
                    file.writeText(content)
                    val virtualFile = LocalFileSystem.getInstance().refreshAndFindFileByIoFile(file)
                    if (virtualFile != null) {
                        val document = FileDocumentManager.getInstance().getDocument(virtualFile)
                        if (document != null) {
                            FileDocumentManager.getInstance().reloadFromDisk(document)
                        }
                    }
                    result = ToolResult.success("Successfully wrote content to $path")
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
        val baseDir = LocalFileSystem.getInstance().findFileByPath(basePath) ?: return ToolResult.error("Project base directory not found")
        val virtualFile = VfsUtil.findRelativeFile(path, baseDir)
            ?: return ToolResult.error("File not found: $path")

        var result: String? = null
        ApplicationManager.getApplication().invokeAndWait {
            WriteCommandAction.runWriteCommandAction(project) {
                val document = FileDocumentManager.getInstance().getDocument(virtualFile)
                if (document == null) {
                    result = ToolResult.error("Could not load document for file: $path")
                    return@runWriteCommandAction
                }
                val content = document.text
                val index = content.indexOf(oldStr)
                if (index == -1) {
                    result = ToolResult.error("String not found in file: $oldStr")
                    return@runWriteCommandAction
                }
                document.replaceString(index, index + oldStr.length, newStr)
                FileDocumentManager.getInstance().saveDocument(document)
                result = ToolResult.success("Successfully replaced content in $path")
            }
        }
        return result ?: ToolResult.error("Unknown error replacing in file")
    }

    fun handleDeleteFile(project: Project, args: JsonObject): String {
        val path = args.get("path").asString
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        val baseDir = LocalFileSystem.getInstance().findFileByPath(basePath) ?: return ToolResult.error("Project base directory not found")
        val virtualFile = VfsUtil.findRelativeFile(path, baseDir)
            ?: return ToolResult.error("File not found: $path")

        var result: String? = null
        ApplicationManager.getApplication().invokeAndWait {
            WriteCommandAction.runWriteCommandAction(project) {
                try {
                    virtualFile.delete(this)
                    result = ToolResult.success("Successfully deleted $path")
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
        val baseDir = LocalFileSystem.getInstance().findFileByPath(basePath) ?: return ToolResult.error("Project base directory not found")
        val rootDir = VfsUtil.findRelativeFile(path, baseDir)
            ?: return ToolResult.error("Directory not found: $path")

        val files = mutableListOf<String>()
        val matcher = FileSystems.getDefault().getPathMatcher("glob:$pattern")
        VfsUtilCore.iterateChildrenRecursively(rootDir, null) { file ->
            if (!file.isDirectory) {
                val relativePath = VfsUtilCore.getRelativePath(file, rootDir) ?: file.name
                if (matcher.matches(Paths.get(relativePath))) {
                    files.add(relativePath)
                }
            }
            true
        }
        return ToolResult.success(files.joinToString("\n"))
    }

    fun handleCreateDirectory(project: Project, args: JsonObject): String {
        val path = args.get("path").asString
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        
        var result: String? = null
        ApplicationManager.getApplication().invokeAndWait {
            WriteCommandAction.runWriteCommandAction(project) {
                try {
                    val file = File(basePath, path)
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
        return handleDeleteFile(project, args)
    }

    fun handleDuplicatePath(project: Project, args: JsonObject): String {
        val from = args.get("from").asString
        val to = args.get("to").asString
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        val baseDir = LocalFileSystem.getInstance().findFileByPath(basePath) ?: return ToolResult.error("Project base directory not found")
        val fromFile = VfsUtil.findRelativeFile(from, baseDir)
            ?: return ToolResult.error("Source file not found: $from")
        
        val toPath = Paths.get(basePath, to)
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
        val baseDir = LocalFileSystem.getInstance().findFileByPath(basePath) ?: return ToolResult.error("Project base directory not found")
        val virtualFile = VfsUtil.findRelativeFile(from, baseDir)
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
        val glob = args.get("glob")?.asString
        
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        val baseDir = LocalFileSystem.getInstance().findFileByPath(basePath) ?: return ToolResult.error("Project base directory not found")
        val rootDir = VfsUtil.findRelativeFile(path, baseDir)
            ?: return ToolResult.error("Directory not found: $path")

        val results = mutableListOf<String>()
        val regex = if (caseInsensitive) Regex(pattern, RegexOption.IGNORE_CASE) else Regex(pattern)
        val globMatcher = glob?.let { FileSystems.getDefault().getPathMatcher("glob:$it") }

        VfsUtilCore.iterateChildrenRecursively(rootDir, null) { file ->
            if (!file.isDirectory) {
                val relativePath = VfsUtilCore.getRelativePath(file, rootDir) ?: file.name
                if (globMatcher == null || globMatcher.matches(Paths.get(relativePath))) {
                    val document = FileDocumentManager.getInstance().getDocument(file)
                    document?.text?.lines()?.forEachIndexed { index, line ->
                        if (regex.containsMatchIn(line)) {
                            results.add("$relativePath:${index + 1}: $line")
                        }
                    }
                }
            }
            true
        }
        return ToolResult.success(results.joinToString("\n"))
    }

    fun handleSearchInFile(project: Project, args: JsonObject): String {
        val path = args.get("path").asString
        val query = args.get("question").asString
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        val baseDir = LocalFileSystem.getInstance().findFileByPath(basePath) ?: return ToolResult.error("Project base directory not found")
        val file = VfsUtil.findRelativeFile(path, baseDir)
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
        return ToolResult.success("Patch received and processing initiated (simulated)")
    }
}
