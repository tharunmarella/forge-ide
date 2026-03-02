# Forge AI Plugin for JetBrains IDEs

An AI-powered coding assistant plugin for **all JetBrains IDEs** that provides intelligent code analysis, generation, and refactoring capabilities through a native chat interface.

**Compatible with**: IntelliJ IDEA, PyCharm, WebStorm, PhpStorm, GoLand, RubyMine, CLion, Rider, DataGrip, Android Studio, and more.

## 🚀 Features

- **Native IntelliJ UI**: Seamlessly integrated chat interface using native Swing components
- **Real-time Streaming**: Server-Sent Events (SSE) for responsive AI interactions
- **Tool Execution**: Automated code analysis, file operations, and project management
- **Syntax Highlighting**: Native code display with IntelliJ's syntax highlighter
- **Diagram Support**: Mermaid diagram rendering in chat
- **Theme Aware**: Automatically matches IntelliJ's dark/light theme

## 📋 Requirements

- **IntelliJ IDEA**: 2023.2.5 or later (Community or Ultimate Edition)
- **Java**: JDK 17 or higher
- **Kotlin**: 1.9.22
- **Backend**: Forge API server (see [forge-search](../forge-search))

## 🛠️ Building the Plugin

### Prerequisites

Ensure you have JDK 17 installed:

```bash
java -version
```

### Build Commands

```bash
# Clean and build the plugin
./gradlew clean build

# Run in sandbox IDE for testing
./gradlew runIde

# Build distributable plugin ZIP
./gradlew buildPlugin
```

The built plugin will be located at:
```
build/distributions/forge-intellij-1.0-SNAPSHOT.zip
```

## 📦 Installation

### From Source

1. Build the plugin (see above)
2. In IntelliJ IDEA: `Settings → Plugins → ⚙️ → Install Plugin from Disk...`
3. Select the built ZIP file
4. Restart IntelliJ IDEA

### Configuration

The plugin connects to the Forge backend API at `http://localhost:8000` by default.

To change the backend URL, modify `ForgeApiService.kt`:
```kotlin
private val baseUrl = "http://localhost:8000"
```

## 🎯 Usage

1. **Open Tool Window**: `View → Tool Windows → Forge AI`
2. **Start Chatting**: Type your request in the input field
3. **View Results**: AI responses stream in real-time with tool execution details

### Example Prompts

- "Analyze the architecture of this project"
- "Find all usages of the `UserService` class"
- "Refactor this method to use async/await"
- "Show me a diagram of the class hierarchy"
- "Write unit tests for the current file"

## 🏗️ Architecture

### Project Structure

```
forge-intellij/
├── src/main/kotlin/com/forge/plugin/
│   ├── api/                    # Backend communication
│   │   ├── ForgeApiService.kt  # HTTP/SSE client
│   │   ├── SseEvent.kt         # Event models
│   │   ├── ToolExecutor.kt     # Tool execution logic
│   │   ├── ToolHandler.kt      # Tool routing
│   │   └── handlers/           # Individual tool handlers
│   └── ui/                     # User interface
│       ├── ForgeToolWindowFactory.kt
│       ├── ForgeChatPanel.kt   # Native Swing chat UI
│       └── ForgeUIService.kt   # UI event bridge
├── docs/                       # Documentation
├── scripts/                    # Test and analysis scripts
└── build.gradle.kts
```

### Key Components

#### 1. **ForgeApiService**
Handles HTTP requests and SSE streaming to the backend:
- Sends chat messages
- Receives streaming events
- Manages connection lifecycle

#### 2. **ToolExecutor**
Executes tools requested by the AI:
- File operations (read, write, search)
- Code analysis (symbols, references)
- Project operations (build, test, run)
- Special rendering (diagrams, code blocks)

#### 3. **ForgeChatPanel**
Native Swing UI for chat interaction:
- Real-time message streaming
- Tool execution visualization
- Syntax-highlighted code display
- Mermaid diagram rendering

## 🔧 Development

### Running Tests

```bash
# Run all tests
./gradlew test

# Run specific test
./gradlew test --tests "com.forge.plugin.api.ForgeApiServiceTest"
```

### Debug Mode

Run the plugin in debug mode:
```bash
./gradlew runIde --debug-jvm
```

Then attach a remote debugger to port 5005.

### Code Style

The project follows Kotlin coding conventions. Format code with:
```bash
./gradlew ktlintFormat
```

## 📚 Documentation

- **[UI Changes](docs/UI_CHANGES.md)**: Native Swing UI implementation details
- **[Gemini Fix Summary](docs/GEMINI_FIX_SUMMARY.md)**: Tool calling fixes for Gemini models
- **[Performance Optimization](docs/PERFORMANCE_OPTIMIZATION.md)**: Latency improvements
- **[Prompt Optimization](docs/PROMPT_OPTIMIZATION.md)**: AI prompt engineering
- **[Tool API Usage](docs/TOOL_API_USAGE.md)**: Backend tool API reference
- **[Tool Result Format](docs/TOOL_RESULT_FORMAT.md)**: Event format specifications

## 🐛 Troubleshooting

### Plugin Not Loading

1. Check IntelliJ version compatibility (2023.2+)
2. Verify JDK 17 is configured
3. Check IDE logs: `Help → Show Log in Finder/Explorer`

### Backend Connection Issues

1. Ensure Forge API is running: `curl http://localhost:8000/health`
2. Check firewall settings
3. Review plugin logs for connection errors

### Tool Execution Failures

1. Verify project is properly indexed
2. Check file permissions
3. Review tool execution logs in the chat panel

## 🤝 Contributing

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/my-feature`
3. Commit changes: `git commit -am 'Add new feature'`
4. Push to branch: `git push origin feature/my-feature`
5. Submit a pull request

## 📄 License

[Add your license here]

## 🔗 Related Projects

- **[forge-search](../forge-search)**: Backend API server with LangChain agent
- **Forge VSCode Extension**: (if applicable)

## 📞 Support

For issues and questions:
- GitHub Issues: [Create an issue](../../issues)
- Documentation: [docs/](docs/)

---

**Built with ❤️ using IntelliJ Platform SDK**
