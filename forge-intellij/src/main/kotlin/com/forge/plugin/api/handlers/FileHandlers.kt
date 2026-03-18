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

        var result: String? = null
        ApplicationManager.getApplication().invokeAndWait {
            WriteCommandAction.runWriteCommandAction(project) {
                try {
                    val file = File(basePath, path)
                    file.parentFile?.mkdirs()
                    // Refresh parent via VFS so IntelliJ tracks the new file properly
                    val parentVFile = LocalFileSystem.getInstance()
                        .refreshAndFindFileByIoFile(file.parentFile)
                        ?: return@runWriteCommandAction run { result = ToolResult.error("Parent directory not found") }
                    val vFile = LocalFileSystem.getInstance().findFileByIoFile(file)
                        ?: parentVFile.createChildData(this, file.name)
                    VfsUtil.saveText(vFile, content)
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
        val contextLines = (args.get("context")?.asInt ?: 0).coerceIn(0, 5)

        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        val baseDir = LocalFileSystem.getInstance().findFileByPath(basePath)
            ?: return ToolResult.error("Project base directory not found")
        val rootDir = VfsUtil.findRelativeFile(path, baseDir)
            ?: return ToolResult.error("Directory not found: $path")

        val results = mutableListOf<String>()
        val regex = if (caseInsensitive) Regex(pattern, RegexOption.IGNORE_CASE) else Regex(pattern)
        val globMatcher = glob?.let { FileSystems.getDefault().getPathMatcher("glob:$it") }

        VfsUtilCore.iterateChildrenRecursively(rootDir, { dir ->
            // Skip directories that are never useful to search
            dir.name !in SKIP_DIRS
        }) { file ->
            if (!file.isDirectory) {
                val relativePath = VfsUtilCore.getRelativePath(file, rootDir) ?: file.name
                if ((globMatcher == null || globMatcher.matches(Paths.get(relativePath)))
                    && !file.fileType.isBinary) {
                    val document = FileDocumentManager.getInstance().getDocument(file)
                    document?.text?.lines()?.forEachIndexed { index, line ->
                        if (regex.containsMatchIn(line)) {
                            if (contextLines > 0) {
                                val lines = document.text.lines()
                                val start = (index - contextLines).coerceAtLeast(0)
                                val end = (index + contextLines).coerceAtMost(lines.size - 1)
                                for (i in start..end) {
                                    val sep = if (i == index) ":" else "-"
                                    results.add("$relativePath:${i + 1}$sep ${lines[i]}")
                                }
                                results.add("--")
                            } else {
                                results.add("$relativePath:${index + 1}: $line")
                            }
                        }
                    }
                }
            }
            results.size < 500 // stop after 500 matches to avoid OOM
        }
        return ToolResult.success(results.joinToString("\n"))
    }

    companion object {
        private val SKIP_DIRS = setOf(
            "node_modules", ".git", "target", "build", "dist", ".idea",
            "__pycache__", ".gradle", ".next", "out", "coverage", ".DS_Store"
        )
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
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")

        // V4A multi-file format (*** Begin Patch / *** Update File: ...)
        val v4aInput = args.get("input")?.asString
        if (v4aInput != null) {
            return applyV4APatch(project, basePath, v4aInput)
        }

        // Unified diff format (single file: path + patch)
        val patchText = args.get("patch")?.asString ?: return ToolResult.error("Missing 'patch' or 'input' argument")
        val filePath = args.get("path")?.asString ?: return ToolResult.error("Missing 'path' for unified diff patch")
        return applyUnifiedDiff(project, basePath, filePath, patchText)
    }

    private fun applyV4APatch(project: Project, basePath: String, input: String): String {
        val lines = input.lines()
        val errors = mutableListOf<String>()
        var currentFile: String? = null
        val fileHunks = mutableMapOf<String, MutableList<String>>()

        for (line in lines) {
            when {
                line.startsWith("*** Update File:") -> {
                    currentFile = line.removePrefix("*** Update File:").trim()
                    fileHunks.getOrPut(currentFile) { mutableListOf() }
                }
                line.startsWith("*** Begin Patch") || line.startsWith("*** End Patch") -> { /* skip markers */ }
                currentFile != null -> fileHunks[currentFile]!!.add(line)
            }
        }

        for ((filePath, hunkLines) in fileHunks) {
            val err = applyUnifiedDiff(project, basePath, filePath, hunkLines.joinToString("\n"))
            if (err.contains("\"success\":false")) errors.add("$filePath: $err")
        }

        return if (errors.isEmpty()) ToolResult.success("Applied patch to ${fileHunks.size} file(s)")
        else ToolResult.error("Patch partially failed:\n${errors.joinToString("\n")}")
    }

    private fun applyUnifiedDiff(project: Project, basePath: String, filePath: String, patch: String): String {
        var result: String? = null
        ApplicationManager.getApplication().invokeAndWait {
            WriteCommandAction.runWriteCommandAction(project) {
                try {
                    val file = File(basePath, filePath)
                    if (!file.exists()) {
                        // New file — collect all added lines
                        val newContent = patch.lines()
                            .filter { it.startsWith("+") && !it.startsWith("+++") }
                            .joinToString("\n") { it.removePrefix("+") }
                        file.parentFile?.mkdirs()
                        val parentVFile = LocalFileSystem.getInstance().refreshAndFindFileByIoFile(file.parentFile)
                        if (parentVFile != null) {
                            val vFile = parentVFile.createChildData(this, file.name)
                            VfsUtil.saveText(vFile, newContent)
                        }
                        result = ToolResult.success("Created $filePath")
                        return@runWriteCommandAction
                    }

                    val vFile = LocalFileSystem.getInstance().findFileByIoFile(file)
                        ?: return@runWriteCommandAction run { result = ToolResult.error("VirtualFile not found: $filePath") }
                    val document = FileDocumentManager.getInstance().getDocument(vFile)
                        ?: return@runWriteCommandAction run { result = ToolResult.error("Cannot get document: $filePath") }

                    val originalLines = document.text.lines().toMutableList()
                    val patchLines = patch.lines()
                    var i = 0
                    var lineOffset = 0 // tracks shifts from prior hunks

                    while (i < patchLines.size) {
                        val line = patchLines[i]
                        if (line.startsWith("@@")) {
                            // Parse @@ -start,count +start,count @@
                            val match = Regex("""-(\d+)(?:,\d+)?\s+\+(\d+)""").find(line)
                            var origLine = (match?.groupValues?.get(1)?.toIntOrNull() ?: 1) - 1 + lineOffset
                            i++
                            val hunkRemoves = mutableListOf<Int>()
                            val hunkAdds = mutableListOf<String>()
                            var hunkPos = origLine
                            while (i < patchLines.size && !patchLines[i].startsWith("@@") &&
                                !patchLines[i].startsWith("*** ")) {
                                val hline = patchLines[i]
                                when {
                                    hline.startsWith("-") -> { hunkRemoves.add(hunkPos); hunkPos++ }
                                    hline.startsWith("+") -> hunkAdds.add(hline.removePrefix("+"))
                                    else -> hunkPos++ // context line
                                }
                                i++
                            }
                            // Apply: remove first, then insert
                            val removeSet = hunkRemoves.toSortedSet(compareByDescending { it })
                            for (idx in removeSet) {
                                if (idx in originalLines.indices) originalLines.removeAt(idx)
                            }
                            val insertAt = (hunkRemoves.minOrNull() ?: hunkPos).coerceIn(0, originalLines.size)
                            originalLines.addAll(insertAt, hunkAdds)
                            lineOffset += hunkAdds.size - hunkRemoves.size
                        } else {
                            i++
                        }
                    }

                    document.setText(originalLines.joinToString("\n"))
                    FileDocumentManager.getInstance().saveDocument(document)
                    result = ToolResult.success("Patched $filePath")
                } catch (e: Exception) {
                    result = ToolResult.error("Failed to apply patch to $filePath: ${e.message}")
                }
            }
        }
        return result ?: ToolResult.error("Unknown error applying patch")
    }
}
