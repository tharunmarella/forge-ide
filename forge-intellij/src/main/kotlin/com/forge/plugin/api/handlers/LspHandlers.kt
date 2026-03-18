package com.forge.plugin.api.handlers

import com.google.gson.JsonArray
import com.google.gson.JsonObject
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.project.Project
import com.intellij.psi.*
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.psi.search.searches.MethodReferencesSearch
import com.intellij.psi.stubs.StubIndex
import com.intellij.psi.stubs.StubIndexKey
import com.intellij.psi.util.PsiTreeUtil
import com.forge.plugin.api.ToolResult

object LspHandlers {

    fun handleGetSymbolDefinition(project: Project, args: JsonObject): String {
        val symbol = args.get("symbol").asString
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        
        var resultJson = ToolResult.error("Could not find symbol definition")

        ApplicationManager.getApplication().runReadAction {
            try {
                val scope = GlobalSearchScope.projectScope(project)
                val elements = StubIndex.getElements(
                    StubIndexKey.createIndexKey<String, PsiNamedElement>("java.class.shortname"),
                    symbol, project, scope, PsiNamedElement::class.java
                )

                val resultsArray = JsonArray()
                elements.forEach { element ->
                    val file = element.containingFile?.virtualFile ?: return@forEach
                    val doc = FileDocumentManager.getInstance().getDocument(file) ?: return@forEach
                    
                    val obj = JsonObject()
                    obj.addProperty("path", file.path.removePrefix("$basePath/"))
                    obj.addProperty("line", doc.getLineNumber(element.textOffset))
                    obj.addProperty("content", element.text)
                    resultsArray.add(obj)
                }
                
                val finalObj = JsonObject()
                finalObj.add("definitions", resultsArray)
                resultJson = ToolResult.success(finalObj)
            } catch (e: Exception) {
                resultJson = ToolResult.error("Get symbol definition exception: ${e.message}")
            }
        }
        return resultJson
    }

    fun handleAnalyzeSymbolFlow(project: Project, args: JsonObject): String {
        val symbol = args.get("symbol_name").asString
        val mode = args.get("mode")?.asString ?: "trace"
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        
        var resultJson = ToolResult.error("Could not analyze symbol flow")

        ApplicationManager.getApplication().runReadAction {
            try {
                val scope = GlobalSearchScope.projectScope(project)
                val namedElements = StubIndex.getElements(
                    StubIndexKey.createIndexKey<String, PsiNamedElement>("java.class.shortname"),
                    symbol, project, scope, PsiNamedElement::class.java
                )
                
                val flowArray = JsonArray()
                namedElements.forEach { element ->
                    if (mode == "trace") {
                        val callers = MethodReferencesSearch.search(element as PsiMethod, scope, true)
                        callers.forEach { ref ->
                            val caller = PsiTreeUtil.getParentOfType(ref.element, PsiMethod::class.java)
                            if (caller != null) {
                                val obj = JsonObject()
                                obj.addProperty("caller", caller.name)
                                obj.addProperty("path", caller.containingFile.virtualFile.path.removePrefix("$basePath/"))
                                flowArray.add(obj)
                            }
                        }
                    } else {
                        element.accept(object : JavaRecursiveElementWalkingVisitor() {
                            override fun visitMethodCallExpression(expression: PsiMethodCallExpression) {
                                val method = expression.resolveMethod()
                                if (method != null) {
                                    val obj = JsonObject()
                                    obj.addProperty("callee", method.name)
                                    obj.addProperty("path", method.containingFile.virtualFile.path.removePrefix("$basePath/"))
                                    flowArray.add(obj)
                                }
                                super.visitMethodCallExpression(expression)
                            }
                        })
                    }
                }
                
                val finalObj = JsonObject()
                finalObj.add("flow", flowArray)
                resultJson = ToolResult.success(finalObj)
            } catch (e: Exception) {
                resultJson = ToolResult.error("Analyze flow exception: ${e.message}")
            }
        }
        return resultJson
    }

    private fun findVirtualFile(project: Project, basePath: String, path: String): com.intellij.openapi.vfs.VirtualFile? {
        val file = com.intellij.openapi.vfs.LocalFileSystem.getInstance().findFileByPath("$basePath/$path")
        if (file != null) return file
        
        // Fallback for tests using LightVirtualFileSystem
        return com.intellij.openapi.vfs.VirtualFileManager.getInstance().findFileByUrl("temp:///src/$path")
    }

    fun handleLspGoToDefinition(project: Project, args: JsonObject): String {
        val path = args.get("path").asString
        val line = args.get("line").asInt
        val column = args.get("column").asInt
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")

        var resultJson = ToolResult.error("Failed to find definition")

        ApplicationManager.getApplication().runReadAction {
            try {
                val virtualFile = findVirtualFile(project, basePath, path) ?: return@runReadAction
                val document = FileDocumentManager.getInstance().getDocument(virtualFile) ?: return@runReadAction
                
                // Ensure line/column are within bounds
                if (line < 1 || line > document.lineCount) return@runReadAction
                val lineStart = document.getLineStartOffset(line - 1)
                val lineEnd = document.getLineEndOffset(line - 1)
                val offset = (lineStart + column - 1).coerceIn(lineStart, lineEnd)

                val psiFile = PsiManager.getInstance(project).findFile(virtualFile) ?: return@runReadAction
                val element = psiFile.findElementAt(offset) ?: return@runReadAction
                val target = element.reference?.resolve() ?: element

                val targetFile = target.containingFile?.virtualFile ?: return@runReadAction
                val targetDoc = FileDocumentManager.getInstance().getDocument(targetFile) ?: return@runReadAction
                val targetOffset = target.textOffset
                val targetLine = targetDoc.getLineNumber(targetOffset)
                val targetCol = targetOffset - targetDoc.getLineStartOffset(targetLine)

                val res = JsonObject()
                res.addProperty("path", targetFile.path.removePrefix("$basePath/"))
                res.addProperty("line", targetLine + 1) // 1-indexed for this IDE
                res.addProperty("column", targetCol + 1) // 1-indexed for this IDE
                resultJson = ToolResult.success(res)
            } catch (e: Exception) {
                resultJson = ToolResult.error("GoTo definition exception: ${e.message}")
            }
        }
        return resultJson
    }

    fun handleLspFindReferences(project: Project, args: JsonObject): String {
        val path = args.get("path").asString
        val line = args.get("line").asInt
        val column = args.get("column").asInt
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")

        var resultJson = ToolResult.error("Failed to find references")

        ApplicationManager.getApplication().runReadAction {
            try {
                val virtualFile = findVirtualFile(project, basePath, path) ?: return@runReadAction
                val document = FileDocumentManager.getInstance().getDocument(virtualFile) ?: return@runReadAction
                
                if (line < 1 || line > document.lineCount) return@runReadAction
                val offset = document.getLineStartOffset(line - 1) + column - 1

                val psiFile = PsiManager.getInstance(project).findFile(virtualFile) ?: return@runReadAction
                val element = psiFile.findElementAt(offset) ?: return@runReadAction
                val target = element.reference?.resolve() ?: element

                val query = com.intellij.psi.search.searches.ReferencesSearch.search(target)
                val references = query.findAll()

                val resultsArray = JsonArray()
                for (ref in references) {
                    val refFile = ref.element.containingFile?.virtualFile ?: continue
                    val refDoc = FileDocumentManager.getInstance().getDocument(refFile) ?: continue
                    val refOffset = ref.element.textOffset
                    val refLine = refDoc.getLineNumber(refOffset)
                    val refCol = refOffset - refDoc.getLineStartOffset(refLine)

                    val refObj = JsonObject()
                    refObj.addProperty("path", refFile.path.removePrefix("$basePath/"))
                    refObj.addProperty("line", refLine + 1)
                    refObj.addProperty("column", refCol + 1)
                    resultsArray.add(refObj)
                }

                val finalObj = JsonObject()
                finalObj.add("references", resultsArray)
                resultJson = ToolResult.success(finalObj)
            } catch (e: Exception) {
                resultJson = ToolResult.error("Find references exception: ${e.message}")
            }
        }
        return resultJson
    }

    fun handleLspRename(project: Project, args: JsonObject): String {
        val path = args.get("path").asString
        val line = args.get("line").asInt
        val column = args.get("column").asInt
        val newName = args.get("new_name").asString
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")

        var resultJson = ToolResult.error("Failed to rename")

        ApplicationManager.getApplication().invokeAndWait {
            try {
                val virtualFile = findVirtualFile(project, basePath, path) ?: return@invokeAndWait
                val document = FileDocumentManager.getInstance().getDocument(virtualFile) ?: return@invokeAndWait
                
                if (line < 1 || line > document.lineCount) return@invokeAndWait
                val offset = document.getLineStartOffset(line - 1) + column - 1

                val psiFile = PsiManager.getInstance(project).findFile(virtualFile) ?: return@invokeAndWait
                val element = psiFile.findElementAt(offset) ?: return@invokeAndWait
                val target = element.reference?.resolve() ?: element

                com.intellij.openapi.command.WriteCommandAction.runWriteCommandAction(project) {
                    val processor = com.intellij.refactoring.rename.RenameProcessor(project, target, newName, false, false)
                    processor.run()
                    resultJson = ToolResult.success("Successfully renamed symbol to $newName")
                }
            } catch (e: Exception) {
                resultJson = ToolResult.error("Rename exception: ${e.message}")
            }
        }
        return resultJson
    }

    fun handleLspHover(project: Project, args: JsonObject): String {
        val path = args.get("path").asString
        val line = args.get("line").asInt
        val column = args.get("column").asInt
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        
        var resultJson = ToolResult.error("No documentation found at this position")

        ApplicationManager.getApplication().runReadAction {
            try {
                val virtualFile = findVirtualFile(project, basePath, path) ?: return@runReadAction
                val psiFile = PsiManager.getInstance(project).findFile(virtualFile) ?: return@runReadAction
                val document = FileDocumentManager.getInstance().getDocument(virtualFile) ?: return@runReadAction

                if (line < 1 || line > document.lineCount) return@runReadAction
                val offset = document.getLineStartOffset(line - 1) + column - 1
                val element = psiFile.findElementAt(offset) ?: return@runReadAction

                val docManager = com.intellij.codeInsight.documentation.DocumentationManager.getInstance(project)
                val targetElement = docManager.findTargetElement(null, offset, psiFile, element) ?: element
                
                val provider = com.intellij.lang.documentation.DocumentationProvider.providers.firstOrNull { it.generateDoc(targetElement, element) != null }
                val doc = provider?.generateDoc(targetElement, element) ?: docManager.generateDocumentation(targetElement, element, false)
                
                if (doc != null) {
                    val finalObj = JsonObject()
                    finalObj.addProperty("contents", doc)
                    resultJson = ToolResult.success(finalObj)
                }
            } catch (e: Exception) {
                resultJson = ToolResult.error("Hover exception: ${e.message}")
            }
        }
        return resultJson
    }

    fun handleGetDocumentSymbols(project: Project, args: JsonObject): String {
        val path = args.get("path").asString
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")
        val symbols = JsonArray()

        ApplicationManager.getApplication().runReadAction {
            val virtualFile = findVirtualFile(project, basePath, path) ?: return@runReadAction
            val psiFile = PsiManager.getInstance(project).findFile(virtualFile) ?: return@runReadAction
            
            psiFile.accept(object : PsiRecursiveElementVisitor() {
                override fun visitElement(element: PsiElement) {
                    if (element is PsiNamedElement && (element is PsiMethod || element is PsiClass || element is PsiField)) {
                        val obj = JsonObject()
                        obj.addProperty("name", element.name)
                        obj.addProperty("kind", element.javaClass.simpleName)
                        val doc = FileDocumentManager.getInstance().getDocument(virtualFile)
                        if (doc != null) {
                            obj.addProperty("line", doc.getLineNumber(element.textOffset))
                        }
                        symbols.add(obj)
                    }
                    super.visitElement(element)
                }
            })
        }
        val result = JsonObject()
        result.add("symbols", symbols)
        return ToolResult.success(result)
    }

    fun handleGetWorkspaceSymbols(project: Project, args: JsonObject): String {
        return ProjectHandlers.handleWorkspaceSymbols(project, args)
    }

    fun handleGetDiagnostics(project: Project, args: JsonObject): String {
        return ProjectHandlers.handleDiagnostics(project, args)
    }
}