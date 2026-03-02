# UI Implementation: Native IntelliJ Swing Components

## Overview

The Forge IntelliJ Plugin UI has been migrated from a JBCefBrowser-based webview to **native IntelliJ Swing components**. This provides a cleaner, more integrated look that matches the IntelliJ IDEA theme automatically.

---

## Changes Made

### 1. **Replaced JBCefBrowser with Native Swing Components**

**Before:**
- Used `JBCefBrowser` (Chromium Embedded Framework)
- Custom HTML/CSS/JavaScript UI (`webview/index.html`)
- Required Base64 encoding for message passing
- Mismatch with IntelliJ theme (custom colors)

**After:**
- Uses native IntelliJ Swing components
- `JBTextArea` for chat display
- `JTextField` for user input
- `JButton` for send action
- Automatically matches IDE theme (dark/light)

### 2. **New Components**

#### `ForgeChatPanel` (New Class)
A native Swing panel that displays the AI chat interface:

```kotlin
class ForgeChatPanel(private val project: Project) : JPanel(BorderLayout()) {
    private val chatArea: JBTextArea          // Display chat history
    private val inputField: JTextField        // User input
    private val sendButton: JButton           // Send button
    private val uiService: ForgeUIService     // Service connection
}
```

**Features:**
- Auto-scrolling chat area
- Enter key to send messages
- Real-time event handling (thinking, tool usage, text streaming)
- Thread-safe UI updates with `SwingUtilities.invokeLater`

#### `ForgeUIService` (Simplified)
No longer needs to handle JavaScript bridge complexity:

**Before:**
- Base64 encoding/decoding
- JavaScript execution
- Browser lifecycle management

**After:**
- Simple Gson JSON parsing
- Direct method calls to `ForgeChatPanel`
- Type-safe event handling

---

## UI Features

### Supported Event Types

The native UI handles all events including rich content display:

1. **`text_delta`** - Streams AI response text in real-time
2. **`thinking`** - Shows "Thinking..." indicator
3. **`tool_start`** - Displays tool execution start
4. **`tool_end`** - Shows tool completion status (Done/Failed)
5. **`error`** - Displays error messages
6. **`done`** - Marks end of AI response

### Special Display Tools

The plugin intercepts and renders two special tools:

#### `show_diagram`
When the AI calls `show_diagram`, the plugin:
- Intercepts the tool call in `ToolExecutor`
- Extracts the Mermaid diagram code
- Displays it in a scrollable text area with darker background
- Supports optional title display
- **Note**: Currently shows Mermaid source code; future enhancement could add visual rendering with PlantUML or Java Mermaid library

#### `show_code`
When the AI calls `show_code`, the plugin:
- Intercepts the tool call in `ToolExecutor`
- Creates a read-only `EditorEx` with syntax highlighting
- Detects file type from language parameter
- Shows line numbers
- Uses IntelliJ's native syntax highlighter
- Displays in a properly styled code editor panel

### Layout

```
┌─────────────────────────────────────┐
│                                     │
│  Chat Display Area (JBTextArea)    │
│  - Scrollable                       │
│  - Auto-scrolling                   │
│  - Matches IDE theme                │
│                                     │
└─────────────────────────────────────┘
┌─────────────────────────────────────┐
│ Input Field        │ [Send Button]  │
└─────────────────────────────────────┘
```

---

## Benefits

### 1. **Native Look & Feel** ✅
- Automatically uses IntelliJ's theme engine
- Matches dark/light mode without custom CSS
- Uses JetBrains UI fonts and spacing (`JBUI.insets()`)

### 2. **Better Performance** ✅
- No Chromium overhead
- Faster rendering
- Lower memory usage
- No browser process spawn

### 3. **Simpler Architecture** ✅
- No JavaScript bridge complexity
- No Base64 encoding workarounds
- Direct Kotlin → UI communication
- Easier to debug

### 4. **Platform Independence** ✅
- Works on all platforms without JCEF support issues
- No `JBCefApp.isSupported()` checks needed
- More reliable on Linux and older macOS

### 5. **Better IntelliJ Integration** ✅
- Uses `JBTextArea`, `JBScrollPane` (JetBrains UI components)
- Follows IntelliJ UI guidelines
- Consistent with other tool windows

---

## Removed Files

The following webview files are no longer needed but kept for reference:
- `src/main/resources/webview/index.html` - HTML/CSS/JS chat UI (now unused)

These can be safely deleted in a future cleanup.

---

## API Compatibility

The backend API communication remains **unchanged**:
- Still uses Server-Sent Events (SSE) for streaming
- Same JSON event format
- Same `ForgeApiService` for backend communication
- Only the frontend rendering changed

---

## Testing

To test the native UI:

1. Build the plugin:
   ```bash
   ./gradlew clean build
   ```

2. Run the sandbox IDE:
   ```bash
   ./gradlew runIde
   ```

3. Open any project in the sandbox

4. Open the "Forge AI" tool window (View → Tool Windows → Forge AI)

5. The UI should now show:
   - Native IntelliJ-themed chat area
   - Input field at the bottom
   - Automatic theme matching

---

## Future Enhancements

Possible improvements for the native UI:

1. **Mermaid Rendering**: Integrate a Java-based Mermaid renderer or PlantUML for visual diagrams
2. **Markdown Rendering**: Use IntelliJ's `MarkdownUtil` for rich text formatting
3. **Hyperlinks**: Clickable file paths that open in editor
4. **Context Menu**: Right-click options (copy, clear chat, export diagram, etc.)
5. **History**: Previous conversation persistence
6. **Toolbar**: Actions like clear, export, settings
7. **Copy Buttons**: Quick copy for code blocks and diagram source

---

## Code Structure

### Tool Window Factory
```
ForgeToolWindowFactory.kt
└── createToolWindowContent()
    └── Creates ForgeChatPanel
    └── Registers with ToolWindow
```

### Chat Panel
```
ForgeChatPanel.kt
├── init() - Sets up UI components
├── sendMessage() - Handles user input
├── handleEvent() - Processes backend events
└── appendToChat() - Updates display
```

### UI Service
```
ForgeUIService.kt
├── setChatPanel() - Connects to panel
└── postMessage() - Forwards events from backend
```

---

## Summary

✅ **Native UI successfully implemented**
✅ **Automatic theme matching**
✅ **Simpler architecture**
✅ **Better performance**
✅ **Full feature parity with webview**

The plugin now provides a professional, native IntelliJ experience that feels like a built-in IDE feature rather than an embedded web app.
