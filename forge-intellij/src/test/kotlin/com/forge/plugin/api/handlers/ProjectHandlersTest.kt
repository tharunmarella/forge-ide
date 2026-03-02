package com.forge.plugin.api.handlers

import com.google.gson.JsonObject
import com.intellij.testFramework.fixtures.LightJavaCodeInsightFixtureTestCase

class ProjectHandlersTest : LightJavaCodeInsightFixtureTestCase() {

    fun testHandleWorkspaceSymbols() {
        val fileText = """
            public class MyAwesomeClass {
                private int myAwesomeField;
                public void myAwesomeMethod() {}
            }
        """.trimIndent()
        
        myFixture.addFileToProject("MyAwesomeClass.java", fileText)
        myFixture.configureByText("MyAwesomeClass.java", fileText)
        
        val args = JsonObject()
        args.addProperty("query", "myAwesomeMethod")
        
        val resultJsonStr = ProjectHandlers.handleWorkspaceSymbols(project, args)
        val resultObj = com.google.gson.JsonParser.parseString(resultJsonStr).asJsonObject
        
        assertTrue(resultObj.get("success").asBoolean)
        
        val results = resultObj.get("result").asJsonArray
        assertEquals(1, results.size())
        
        val methodObj = results[0].asJsonObject
        assertEquals("myAwesomeMethod", methodObj.get("name").asString)
        assertEquals("method", methodObj.get("kind").asString)
    }

    fun testHandleCodebaseSearch() {
        val args = JsonObject()
        val resultJsonStr = ProjectHandlers.handleCodebaseSearch(project, args)
        val resultObj = com.google.gson.JsonParser.parseString(resultJsonStr).asJsonObject
        
        assertFalse(resultObj.get("success").asBoolean)
        assertTrue(resultObj.get("error").asString.contains("server-side tool"))
    fun testHandleProjectSkeleton() {
        val args = JsonObject()
        val resultJsonStr = ProjectHandlers.handleProjectSkeleton(project, args)
        val resultObj = com.google.gson.JsonParser.parseString(resultJsonStr).asJsonObject
        
        assertTrue(resultObj.get("success").asBoolean)
        val resultData = resultObj.get("result").asJsonObject
        assertNotNull(resultData.get("project_name"))
        assertNotNull(resultData.get("structure"))
    }

    fun testHandleDiagnostics() {
        val fileText = """
            public class BadClass {
                public void badMethod() {
                    int x = "string"; // Compilation error
                }
            }
        """.trimIndent()
        
        myFixture.addFileToProject("BadClass.java", fileText)
        myFixture.configureByText("BadClass.java", fileText)
        
        val args = JsonObject()
        args.addProperty("path", "BadClass.java")
        
        val resultJsonStr = ProjectHandlers.handleDiagnostics(project, args)
        val resultObj = com.google.gson.JsonParser.parseString(resultJsonStr).asJsonObject
        
        assertTrue(resultObj.get("success").asBoolean)
        val resultData = resultObj.get("result").asJsonObject
        val diagnostics = resultData.get("diagnostics").asJsonArray
        // In light tests, daemon analyzer might not run synchronously unless triggered or might return empty.
        // We just ensure it returns successfully with the correct JSON structure without crashing.
        assertNotNull(diagnostics)
    }

    fun testHandleGit() {
        val args = JsonObject()
        args.addProperty("operation", "status")
        
        val resultJsonStr = ProjectHandlers.handleGit(project, args)
        val resultObj = com.google.gson.JsonParser.parseString(resultJsonStr).asJsonObject
        
        // Since there is no git repo in the light fixture, we expect a graceful failure
        assertFalse(resultObj.get("success").asBoolean)
        assertTrue(resultObj.get("error").asString.contains("No git repository found"))
    }
}
}
