package com.forge.plugin.api.handlers

import com.google.gson.JsonArray
import com.google.gson.JsonObject
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.project.Project
import com.intellij.psi.*
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.psi.search.searches.ReferencesSearch
import com.intellij.psi.util.PsiTreeUtil
import com.forge.plugin.api.ToolResult

object LspHandlers {

    fun handleGetSymbolDefinition(project: Project, args: JsonObject): String {
        val symbol = args.get("symbol").asString
        val basePath = project.basePath ?: return ToolResult.error("Project base path not found")

        var resultJson = ToolResult.error("Could not find symbol definition")

        ApplicationManager.getApplication().runReadAction {
            try {
                val resultsArray = JsonArray()
                val scope = GlobalSearchScope.projectScope(project)

                // Language-agnostic: search all project files via PSI named element walk
                // Try fast path first: PsiShortNamesCache covers Java/Kotlin/Groovy
                val cache = com.intellij.psi.search.PsiShortNamesCache.getInstance(project)
                val fastResults = mutableListOf<PsiNamedElement>()
                try {
                    fastResults.addAll(cache.getClassesByName(symbol, scope))
                    fastResults.addAll(cache.getMethodsByName(symbol, scope))
                    fastResults.addAll(cache.getFieldsByName(symbol, scope))
                } catch (_: Exception) {}

                if (fastResults.isNotEmpty()) {
                    for (element in fastResults) {
                        val file = element.containingFile?.virtualFile ?: continue
                        val doc = FileDocumentManager.getInstance().getDocument(file) ?: continue
                        val obj = JsonObject()
                        obj.addProperty("path", file.path.removePrefix("$basePath/"))
                        obj.addProperty("line", doc.getLineNumber(element.textOffset) + 1)
                        obj.addProperty("name", element.name)
                        resultsArray.add(obj)
                    }
                } else {
                    // Fallback: walk PSI tree of all project files (slower, catches all languages)
                    com.intellij.openapi.roots.ProjectFileIndex.getInstance(project).iterateContent { vFile ->
                        if (!vFile.isDirectory) {
                            val psiFile = PsiManager.getInstance(project).findFile(vFile)
                            psiFile?.accept(object : PsiRecursiveElementVisitor() {
                                override fun visitElement(element: PsiElement) {
                                    if (element is PsiNamedElement && element.name == symbol
                                        && element.reference == null) {
                                        val doc = FileDocumentManager.getInstance().getDocument(vFile)
                                        if (doc != null) {
                                            val obj = JsonObject()
                                            obj.addProperty("path", vFile.path.removePrefix("$basePath/"))
                                            obj.addProperty("line", doc.getLineNumber(element.textOffset) + 1)
                                            obj.addProperty("name", element.name)
                                            resultsArray.add(obj)
                                        }
                                    }
                                    if (resultsArray.size() < 20) super.visitElement(element)
                                }
                            })
                        }
                        resultsArray.size() < 20
                    }
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
                val flowArray = JsonArray()

                // Find the target element using language-agnostic PsiShortNamesCache
                val cache = com.intellij.psi.search.PsiShortNamesCache.getInstance(project)
                val candidates = mutableListOf<PsiNamedElement>()
                try {
                    candidates.addAll(cache.getMethodsByName(symbol, scope))
                    candidates.addAll(cache.getClassesByName(symbol, scope))
                } catch (_: Exception) {}

                for (element in candidates) {
                    if (mode == "trace") {
                        // Find all callers/usages via language-agnostic ReferencesSearch
                        val refs = com.intellij.psi.search.searches.ReferencesSearch.search(element, scope)
                        refs.forEach { ref ->
                            val refFile = ref.element.containingFile?.virtualFile ?: return@forEach
                            val caller = PsiTreeUtil.getParentOfType(ref.element, PsiNamedElement::class.java)
                            val obj = JsonObject()
                            obj.addProperty("caller", caller?.name ?: "<unknown>")
                            obj.addProperty("path", refFile.path.removePrefix("$basePath/"))
                            flowArray.add(obj)
                        }
                    } else {
                        // Find callees: walk body looking for references to other named elements
                        element.accept(object : PsiRecursiveElementVisitor() {
                            override fun visitElement(element: PsiElement) {
                                val resolved = element.reference?.resolve()
                                if (resolved is PsiNamedElement && resolved.name != symbol) {
                                    val file = resolved.containingFile?.virtualFile ?: return
                                    val obj = JsonObject()
                                    obj.addProperty("callee", resolved.name)
                                    obj.addProperty("path", file.path.removePrefix("$basePath/"))
                                    flowArray.add(obj)
                                }
                                if (flowArray.size() < 50) super.visitElement(element)
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

                // Language-agnostic: use DocumentationManager which delegates to the
                // language-specific DocumentationProvider registered for each file type.
                val docManager = com.intellij.codeInsight.documentation.DocumentationManager.getInstance(project)
                val targetElement = docManager.findTargetElement(null, offset, psiFile, element) ?: element
                val doc = docManager.generateDocumentation(targetElement, element, false)

                // Fallback: extract type info from the element's text if no doc is available
                val contents = doc ?: run {
                    val resolved = element.reference?.resolve()
                    resolved?.text?.lines()?.take(5)?.joinToString("\n")
                }

                if (contents != null) {
                    val finalObj = JsonObject()
                    finalObj.addProperty("contents", contents)
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
            val doc = FileDocumentManager.getInstance().getDocument(virtualFile)

            psiFile.accept(object : PsiRecursiveElementVisitor() {
                override fun visitElement(element: PsiElement) {
                    if (element is PsiNamedElement) {
                        val name = element.name?.takeIf { it.isNotBlank() }
                            ?: run { super.visitElement(element); return }
                        // Classify by class name — language-agnostic (Java, Kotlin, Python, JS, Rust etc.)
                        val typeName = element.javaClass.simpleName
                        val kind = when {
                            typeName.contains("Function", true)  -> "function"
                            typeName.contains("Method", true)    -> "method"
                            typeName.contains("Class", true)     -> "class"
                            typeName.contains("Interface", true) -> "interface"
                            typeName.contains("Enum", true)      -> "enum"
                            typeName.contains("Field", true) ||
                            typeName.contains("Property", true)  -> "field"
                            // Skip variables, parameters, labels — too noisy
                            typeName.contains("Variable", true) ||
                            typeName.contains("Parameter", true) ||
                            typeName.contains("Label", true)     -> null
                            else -> null
                        }
                        if (kind != null) {
                            val obj = JsonObject()
                            obj.addProperty("name", name)
                            obj.addProperty("kind", kind)
                            if (doc != null) obj.addProperty("line", doc.getLineNumber(element.textOffset) + 1)
                            symbols.add(obj)
                        }
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