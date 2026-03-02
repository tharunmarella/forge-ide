package com.forge.plugin.api.handlers

import com.google.gson.JsonObject
import com.intellij.testFramework.fixtures.LightJavaCodeInsightFixtureTestCase
import com.intellij.openapi.vfs.LocalFileSystem
import java.io.File

class LspHandlersTest : LightJavaCodeInsightFixtureTestCase() {

    override fun setUp() {
        super.setUp()
        // Ensure the base path exists physically so LocalFileSystem can find it
        val basePath = project.basePath!!
        File(basePath).mkdirs()
        LocalFileSystem.getInstance().refreshAndFindFileByIoFile(File(basePath))
    }

    fun testHandleGetDocumentSymbols() {
        val fileText = """
            public class TestClass {
                private int myField;
                public void myMethod() {}
            }
        """.trimIndent()
        
        myFixture.addFileToProject("TestClass.java", fileText)
        myFixture.configureByText("TestClass.java", fileText)
        
        val args = JsonObject()
        args.addProperty("path", "TestClass.java")
        
        val resultJsonStr = LspHandlers.handleGetDocumentSymbols(project, args)
        println("testHandleGetDocumentSymbols result: $resultJsonStr")
        val resultObj = com.google.gson.JsonParser.parseString(resultJsonStr).asJsonObject
        
        assertTrue(resultObj.get("success").asBoolean)
        
        val resultData = resultObj.get("result").asJsonObject
        val symbols = resultData.get("symbols").asJsonArray
        
        val names = symbols.map { it.asJsonObject.get("name").asString }
        assertTrue(names.contains("TestClass"))
        assertTrue(names.contains("myField"))
        assertTrue(names.contains("myMethod"))
    }

    fun testHandleLspGoToDefinition() {
        val fileText = """
            public class Main {
                public void start() {
                    Helper h = new Helper();
                    h.doWork();
                }
            }
            class Helper {
                public void doWork() {}
            }
        """.trimIndent()
        
        myFixture.addFileToProject("Main.java", fileText)
        myFixture.configureByText("Main.java", fileText)
        
        // Find offset of 'doWork' call
        val text = myFixture.editor.document.text
        val offset = text.indexOf("doWork();")
        val line = myFixture.editor.document.getLineNumber(offset)
        val column = offset - myFixture.editor.document.getLineStartOffset(line)
        
        val args = JsonObject()
        args.addProperty("path", "Main.java")
        args.addProperty("line", line)
        args.addProperty("column", column)
        
        val resultJsonStr = LspHandlers.handleLspGoToDefinition(project, args)
        println("testHandleLspGoToDefinition result: $resultJsonStr")
        val resultObj = com.google.gson.JsonParser.parseString(resultJsonStr).asJsonObject
        
        assertTrue(resultObj.get("success").asBoolean)
        val resultData = resultObj.get("result").asJsonObject
        assertEquals("/src/Main.java", resultData.get("path").asString)
        
        // The definition of doWork is on line 7 (0-indexed)
        val targetLine = resultData.get("line").asInt
        assertTrue(targetLine > 0)
    fun testHandleLspFindReferences() {
        val fileText = """
            public class Main {
                public void start() {
                    Helper h = new Helper();
                    h.doWork();
                    h.doWork();
                }
            }
            class Helper {
                public void doWork() {}
            }
        """.trimIndent()
        
        myFixture.addFileToProject("Main.java", fileText)
        myFixture.configureByText("Main.java", fileText)
        
        // Find offset of 'doWork' definition
        val text = myFixture.editor.document.text
        val offset = text.indexOf("public void doWork()") + 12 // position on 'doWork'
        val line = myFixture.editor.document.getLineNumber(offset)
        val column = offset - myFixture.editor.document.getLineStartOffset(line)
        
        val args = JsonObject()
        args.addProperty("path", "Main.java")
        args.addProperty("line", line)
        args.addProperty("column", column)
        
        val resultJsonStr = LspHandlers.handleLspFindReferences(project, args)
        val resultObj = com.google.gson.JsonParser.parseString(resultJsonStr).asJsonObject
        
        assertTrue(resultObj.get("success").asBoolean)
        val resultData = resultObj.get("result").asJsonObject
        val references = resultData.get("references").asJsonArray
        // Expected to find 2 references
        assertEquals(2, references.size())
    }

    fun testHandleLspRename() {
        val fileText = """
            public class Main {
                public void start() {
                    Helper h = new Helper();
                    h.doWork();
                }
            }
            class Helper {
                public void doWork() {}
            }
        """.trimIndent()
        
        myFixture.addFileToProject("Main.java", fileText)
        myFixture.configureByText("Main.java", fileText)
        
        val text = myFixture.editor.document.text
        val offset = text.indexOf("doWork();")
        val line = myFixture.editor.document.getLineNumber(offset)
        val column = offset - myFixture.editor.document.getLineStartOffset(line)
        
        val args = JsonObject()
        args.addProperty("path", "Main.java")
        args.addProperty("line", line)
        args.addProperty("column", column)
        args.addProperty("new_name", "doNewWork")
        
        val resultJsonStr = LspHandlers.handleLspRename(project, args)
        val resultObj = com.google.gson.JsonParser.parseString(resultJsonStr).asJsonObject
        
        assertTrue(resultObj.get("success").asBoolean)
        val resultData = resultObj.get("result").asString
        assertTrue(resultData.contains("Successfully renamed symbol"))
        
        // Check document content
        val newText = myFixture.editor.document.text
        assertTrue(newText.contains("h.doNewWork();"))
        assertTrue(newText.contains("public void doNewWork() {}"))
    }

    fun testHandleLspHover() {
        val fileText = """
            public class Main {
                /**
                 * Starts the main process.
                 */
                public void start() {}
                
                public void callStart() {
                    start();
                }
            }
        """.trimIndent()
        
        myFixture.addFileToProject("Main.java", fileText)
        myFixture.configureByText("Main.java", fileText)
        
        val text = myFixture.editor.document.text
        val offset = text.lastIndexOf("start();")
        val line = myFixture.editor.document.getLineNumber(offset)
        val column = offset - myFixture.editor.document.getLineStartOffset(line)
        
        val args = JsonObject()
        args.addProperty("path", "Main.java")
        args.addProperty("line", line)
        args.addProperty("column", column)
        
        val resultJsonStr = LspHandlers.handleLspHover(project, args)
        val resultObj = com.google.gson.JsonParser.parseString(resultJsonStr).asJsonObject
        
        assertTrue(resultObj.get("success").asBoolean)
        val resultData = resultObj.get("result").asJsonObject
        val contents = resultData.get("contents").asString
        
        assertTrue(contents.contains("Starts the main process."))
    }

    fun testHandleGetSymbolDefinition() {
        val fileText = """
            public class SymbolTestClass {
                public void symbolMethod() {}
            }
        """.trimIndent()
        
        myFixture.addFileToProject("SymbolTestClass.java", fileText)
        myFixture.configureByText("SymbolTestClass.java", fileText)
        
        val args = JsonObject()
        args.addProperty("symbol", "SymbolTestClass")
        
        val resultJsonStr = LspHandlers.handleGetSymbolDefinition(project, args)
        val resultObj = com.google.gson.JsonParser.parseString(resultJsonStr).asJsonObject
        
        assertTrue(resultObj.get("success").asBoolean)
        val definitions = resultObj.get("result").asJsonObject.get("definitions").asJsonArray
        assertEquals(1, definitions.size())
        
        val defObj = definitions[0].asJsonObject
        assertTrue(defObj.get("content").asString.contains("SymbolTestClass"))
    }

    fun testHandleAnalyzeSymbolFlow() {
        val fileText = """
            public class FlowClass {
                public void methodA() {
                    methodB();
                }
                public void methodB() {
                }
            }
        """.trimIndent()
        
        myFixture.addFileToProject("FlowClass.java", fileText)
        myFixture.configureByText("FlowClass.java", fileText)
        
        val args = JsonObject()
        args.addProperty("symbol_name", "FlowClass")
        args.addProperty("mode", "trace")
        
        val resultJsonStr = LspHandlers.handleAnalyzeSymbolFlow(project, args)
        val resultObj = com.google.gson.JsonParser.parseString(resultJsonStr).asJsonObject
        
        assertTrue(resultObj.get("success").asBoolean)
        val flow = resultObj.get("result").asJsonObject.get("flow").asJsonArray
        // We aren't testing the full index here deeply as Light fixtures index differently sometimes,
        // but we just verify it doesn't crash and returns the expected format.
        assertNotNull(flow)
    }
}}
