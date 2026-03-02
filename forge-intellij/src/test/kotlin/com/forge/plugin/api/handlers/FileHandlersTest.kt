package com.forge.plugin.api.handlers

import com.forge.plugin.api.ToolResult
import com.google.gson.JsonObject
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.openapi.application.ApplicationManager

class FileHandlersTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        val basePath = project.basePath!!
        java.io.File(basePath).mkdirs()
        com.intellij.openapi.vfs.LocalFileSystem.getInstance().refreshAndFindFileByIoFile(java.io.File(basePath))
    }

    fun testHandleReadFile() {
        val basePath = project.basePath!!
        println("testHandleReadFile basePath: $basePath")
        
        // Create physical file in the project directory
        val ioFile = java.io.File(basePath, "test.txt")
        ioFile.writeText("line1\nline2\nline3")
        
        // Ensure VFS catches up
        com.intellij.openapi.vfs.LocalFileSystem.getInstance().refreshAndFindFileByIoFile(ioFile)
        
        val args = JsonObject()
        args.addProperty("path", "test.txt")
        
        val resultJsonStr = FileHandlers.handleReadFile(project, args)
        println("testHandleReadFile result: $resultJsonStr")
        
        // It returns a JSON string, let's parse and check
        val resultObj = com.google.gson.JsonParser.parseString(resultJsonStr).asJsonObject
        
        assertTrue(resultObj.get("success").asBoolean)
        
        val outputObj = resultObj.get("result").asJsonObject
        
        assertEquals("line1\nline2\nline3", outputObj.get("content").asString)
        assertEquals(3, outputObj.get("total_lines").asInt)
    }

    fun testHandleWriteToFile() {
        val basePath = project.basePath!!
        val args = JsonObject()
        args.addProperty("path", "new_test.txt")
        args.addProperty("content", "Hello Forge")
        
        val resultJsonStr = FileHandlers.handleWriteToFile(project, args)
        val resultObj = com.google.gson.JsonParser.parseString(resultJsonStr).asJsonObject
        assertTrue(resultObj.get("success").asBoolean)
        
        val ioFile = java.io.File(basePath, "new_test.txt")
        assertTrue(ioFile.exists())
        assertEquals("Hello Forge", ioFile.readText())
    }

    fun testHandleListFiles() {
        val basePath = project.basePath!!
        val baseDir = java.io.File(basePath)
        baseDir.mkdirs()
        
        java.io.File(basePath, "file1.txt").writeText("content")
        val dir = java.io.File(basePath, "dir")
        dir.mkdirs()
        java.io.File(dir, "file2.txt").writeText("content")
        
        com.intellij.openapi.vfs.LocalFileSystem.getInstance().refreshAndFindFileByIoFile(baseDir)
        
        val args = JsonObject()
        args.addProperty("path", ".")
        args.addProperty("recursive", false)
        
        val resultJsonStr = FileHandlers.handleListFiles(project, args)
        val resultObj = com.google.gson.JsonParser.parseString(resultJsonStr).asJsonObject
        assertTrue(resultObj.get("success").asBoolean)
        
        val outputArray = resultObj.get("result").asJsonArray
        
        val files = outputArray.map { it.asString }
        assertTrue(files.contains("file1.txt"))
        assertFalse(files.contains("dir"))
    }
    
    fun testHandleReplaceInFile() {
        val basePath = project.basePath!!
        val ioFile = java.io.File(basePath, "replace_test.txt")
        ioFile.writeText("Hello world!")
        com.intellij.openapi.vfs.LocalFileSystem.getInstance().refreshAndFindFileByIoFile(ioFile)
        
        val args = JsonObject()
        args.addProperty("path", "replace_test.txt")
        args.addProperty("old_str", "world")
        args.addProperty("new_str", "Forge")
        
        val resultJsonStr = FileHandlers.handleReplaceInFile(project, args)
        val resultObj = com.google.gson.JsonParser.parseString(resultJsonStr).asJsonObject
        assertTrue(resultObj.get("success").asBoolean)
        
        // Let's verify the content via Document since that's what handleReplaceInFile modifies
        val file = com.intellij.openapi.vfs.LocalFileSystem.getInstance().findFileByPath("${project.basePath}/replace_test.txt")!!
        val document = com.intellij.openapi.fileEditor.FileDocumentManager.getInstance().getDocument(file)!!
        assertEquals("Hello Forge!", document.text)
    }

    fun testHandleDeleteFile() {
        val basePath = project.basePath!!
        val ioFile = java.io.File(basePath, "delete_test.txt")
        ioFile.writeText("To be deleted")
        com.intellij.openapi.vfs.LocalFileSystem.getInstance().refreshAndFindFileByIoFile(ioFile)
        
        val args = JsonObject()
        args.addProperty("path", "delete_test.txt")
        
        val resultJsonStr = FileHandlers.handleDeleteFile(project, args)
        val resultObj = com.google.gson.JsonParser.parseString(resultJsonStr).asJsonObject
        assertTrue(resultObj.get("success").asBoolean)
        
        assertFalse(ioFile.exists())
    }

    fun testHandleGlob() {
        val basePath = project.basePath!!
        val f1 = java.io.File(basePath, "test1.kt")
        f1.writeText("")
        val f2 = java.io.File(basePath, "test2.kt")
        f2.writeText("")
        val f3 = java.io.File(basePath, "test3.txt")
        f3.writeText("")
        
        val lfs = com.intellij.openapi.vfs.LocalFileSystem.getInstance()
        lfs.refreshAndFindFileByIoFile(f1)
        lfs.refreshAndFindFileByIoFile(f2)
        lfs.refreshAndFindFileByIoFile(f3)
        lfs.refreshAndFindFileByIoFile(java.io.File(basePath))
        
        val args = JsonObject()
        args.addProperty("pattern", "*.kt")
        args.addProperty("path", ".")
        
        val resultJsonStr = FileHandlers.handleGlob(project, args)
        val resultObj = com.google.gson.JsonParser.parseString(resultJsonStr).asJsonObject
        assertTrue(resultObj.get("success").asBoolean)
        
        val matches = resultObj.get("result").asJsonArray.map { it.asString }
        assertTrue(matches.contains("test1.kt"))
        assertTrue(matches.contains("test2.kt"))
        assertFalse(matches.contains("test3.txt"))
    }

    fun testHandleSearchInFile() {
        val basePath = project.basePath!!
        val ioFile = java.io.File(basePath, "search_test.txt")
        ioFile.writeText("Line 1\nTarget line\nLine 3")
        com.intellij.openapi.vfs.LocalFileSystem.getInstance().refreshAndFindFileByIoFile(ioFile)
        
        val args = JsonObject()
        args.addProperty("path", "search_test.txt")
        args.addProperty("question", "Target")
        
        val resultJsonStr = FileHandlers.handleSearchInFile(project, args)
        val resultObj = com.google.gson.JsonParser.parseString(resultJsonStr).asJsonObject
        assertTrue(resultObj.get("success").asBoolean)
        
        val matches = resultObj.get("result").asJsonObject.get("matches").asJsonArray
        assertEquals(1, matches.size())
        assertEquals("Target line", matches[0].asJsonObject.get("content").asString)
        assertEquals(2, matches[0].asJsonObject.get("line").asInt)
    }

    fun testHandleGrep() {
        val basePath = project.basePath!!
        val f1 = java.io.File(basePath, "grep1.txt")
        f1.writeText("Find this string here")
        val f2 = java.io.File(basePath, "grep2.txt")
        f2.writeText("Nothing here")
        val f3 = java.io.File(basePath, "grep3.txt")
        f3.writeText("Also FIND this")
        
        val lfs = com.intellij.openapi.vfs.LocalFileSystem.getInstance()
        lfs.refreshAndFindFileByIoFile(f1)
        lfs.refreshAndFindFileByIoFile(f2)
        lfs.refreshAndFindFileByIoFile(f3)
        lfs.refreshAndFindFileByIoFile(java.io.File(basePath))
        
        val args = JsonObject()
        args.addProperty("pattern", "Find this")
        args.addProperty("path", ".")
        args.addProperty("case_insensitive", true)
        
        val resultJsonStr = FileHandlers.handleGrep(project, args)
        val resultObj = com.google.gson.JsonParser.parseString(resultJsonStr).asJsonObject
        assertTrue(resultObj.get("success").asBoolean)
        
        val results = resultObj.get("result").asJsonArray
        assertEquals(2, results.size())
        
        val contents = results.map { it.asJsonObject.get("content").asString }
        assertTrue(contents.contains("Find this string here"))
        assertTrue(contents.contains("Also FIND this"))
    }
}
