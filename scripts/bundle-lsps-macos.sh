#!/bin/bash
# Bundle language servers into a macOS .app bundle (arm64).
# Usage: ./scripts/bundle-lsps-macos.sh /path/to/Forge-IDE.app
set -euo pipefail

if [ $# -lt 1 ]; then
    echo "Usage: $0 /path/to/Forge-IDE.app"
    exit 1
fi

APP_DIR="$1"
ARCH="arm64"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Pinned versions (override via env for reproducible CI builds)
NODE_VERSION="${NODE_VERSION:-22.16.0}"
PYRIGHT_VERSION="${PYRIGHT_VERSION:-1.1.400}"
TLSLS_VERSION="${TLSLS_VERSION:-5.3.0}"
TYPESCRIPT_VERSION="${TYPESCRIPT_VERSION:-5.8.3}"
GOPLS_VERSION="${GOPLS_VERSION:-v0.22.0}"
RUST_ANALYZER_TAG="${RUST_ANALYZER_TAG:-}"

LSP_DIR="$APP_DIR/Contents/Resources/lsp"
MACOS_DIR="$APP_DIR/Contents/MacOS"
NODE_DIR="$LSP_DIR/node-v${NODE_VERSION}-darwin-${ARCH}"

if [ ! -d "$APP_DIR/Contents" ]; then
    echo "ERROR: Not a valid .app bundle: $APP_DIR"
    exit 1
fi

mkdir -p "$LSP_DIR" "$MACOS_DIR"

ensure_node() {
    if [ -x "$NODE_DIR/bin/node" ]; then
        sign_bundled_node
        return 0
    fi
    if ! command -v curl >/dev/null 2>&1; then
        echo "ERROR: curl is required to download Node.js"
        exit 1
    fi
    local tmp tarball
    tmp=$(mktemp -d)
    tarball="node-v${NODE_VERSION}-darwin-${ARCH}.tar.gz"
    echo "    Downloading Node.js ${NODE_VERSION}..."
    curl -fsSL "https://nodejs.org/dist/v${NODE_VERSION}/${tarball}" | tar xz -C "$tmp"
    rm -rf "$NODE_DIR"
    mv "$tmp/node-v${NODE_VERSION}-darwin-${ARCH}" "$NODE_DIR"
    rm -rf "$tmp"
    sign_bundled_node
}

sign_bundled_node() {
    local node_bin="$NODE_DIR/bin/node"
    local ents="$SCRIPT_DIR/../extra/entitlements.plist"
    if [ ! -x "$node_bin" ] || [ ! -f "$ents" ]; then
        return 0
    fi
    echo "    Signing bundled Node with JIT entitlements..."
    codesign --force --sign - --options runtime --entitlements "$ents" "$node_bin" 2>/dev/null \
        || codesign --force --sign - --entitlements "$ents" "$node_bin" 2>/dev/null \
        || echo "    WARNING: could not codesign bundled Node"
}

write_node_wrapper() {
    local name="$1"
    local entry_js="$2"
    cp "$SCRIPT_DIR/lsp/node-unbuffer.cjs" "$LSP_DIR/node-unbuffer.cjs"
    cat > "$MACOS_DIR/$name" << EOF
#!/bin/bash
set -euo pipefail
LSP_ROOT="\$(cd "\$(dirname "\$0")/../Resources/lsp" && pwd)"
BUNDLED_NODE="\$LSP_ROOT/node-v${NODE_VERSION}-darwin-${ARCH}/bin/node"
NODE=""
for candidate in /opt/homebrew/bin/node /usr/local/bin/node "\$(command -v node 2>/dev/null || true)"; do
  if [ -n "\$candidate" ] && [ -x "\$candidate" ]; then
    NODE="\$candidate"
    break
  fi
done
if [ -z "\$NODE" ]; then
  NODE="\$BUNDLED_NODE"
fi
# Node buffers stdout when piped; unbuffer only works with a functional Node runtime.
if [ "\$NODE" != "\$BUNDLED_NODE" ] && [ -f "\$LSP_ROOT/node-unbuffer.cjs" ]; then
  export NODE_OPTIONS="--require \$LSP_ROOT/node-unbuffer.cjs\${NODE_OPTIONS:+ \$NODE_OPTIONS}"
fi
exec "\$NODE" "\$LSP_ROOT/${entry_js}" "\$@"
EOF
    chmod +x "$MACOS_DIR/$name"
}

bundle_pyright() {
    if ! command -v npm >/dev/null 2>&1; then
        echo "WARNING: npm not found — skipping Pyright bundle"
        return 0
    fi

    echo "==> Bundling Pyright v${PYRIGHT_VERSION}"
    ensure_node

    local pyright_dir="$LSP_DIR/pyright"
    rm -rf "$pyright_dir"
    mkdir -p "$pyright_dir"
    (
        cd "$pyright_dir"
        npm init -y >/dev/null 2>&1
        npm install "pyright@${PYRIGHT_VERSION}" --omit=dev --no-fund --no-audit >/dev/null 2>&1
    )

    local langserver="$pyright_dir/node_modules/pyright/langserver.index.js"
    if [ ! -f "$langserver" ]; then
        echo "ERROR: Pyright langserver entry point not found"
        exit 1
    fi

    write_node_wrapper "pyright-langserver" "pyright/node_modules/pyright/langserver.index.js"
    echo "    Wrapper: $MACOS_DIR/pyright-langserver"
}

bundle_typescript_language_server() {
    if ! command -v npm >/dev/null 2>&1; then
        echo "WARNING: npm not found — skipping TypeScript language server bundle"
        return 0
    fi

    echo "==> Bundling typescript-language-server v${TLSLS_VERSION}"
    ensure_node

    local tsls_dir="$LSP_DIR/typescript-language-server"
    rm -rf "$tsls_dir"
    mkdir -p "$tsls_dir"
    (
        cd "$tsls_dir"
        npm init -y >/dev/null 2>&1
        npm install \
            "typescript-language-server@${TLSLS_VERSION}" \
            "typescript@${TYPESCRIPT_VERSION}" \
            --omit=dev --no-fund --no-audit >/dev/null 2>&1
    )

    local cli="$tsls_dir/node_modules/typescript-language-server/lib/cli.mjs"
    if [ ! -f "$cli" ]; then
        echo "ERROR: typescript-language-server entry point not found"
        exit 1
    fi

    write_node_wrapper "typescript-language-server" "typescript-language-server/node_modules/typescript-language-server/lib/cli.mjs"
    echo "    Wrapper: $MACOS_DIR/typescript-language-server"
}

bundle_rust_analyzer() {
    echo "==> Bundling rust-analyzer"

    local tag="$RUST_ANALYZER_TAG"
    if [ -z "$tag" ]; then
        tag=$(curl -fsSL "https://api.github.com/repos/rust-lang/rust-analyzer/releases/latest" \
            | python3 -c "import sys,json; print(json.load(sys.stdin)['tag_name'])")
    fi

    local asset="rust-analyzer-aarch64-apple-darwin.gz"
    local url="https://github.com/rust-lang/rust-analyzer/releases/download/${tag}/${asset}"
    echo "    Downloading rust-analyzer ${tag}..."
    curl -fsSL "$url" | gunzip > "$MACOS_DIR/rust-analyzer"
    chmod +x "$MACOS_DIR/rust-analyzer"

    if ! "$MACOS_DIR/rust-analyzer" --version >/dev/null 2>&1; then
        echo "ERROR: Bundled rust-analyzer failed to run"
        exit 1
    fi
    echo "    Binary: $MACOS_DIR/rust-analyzer ($("$MACOS_DIR/rust-analyzer" --version 2>&1 | head -1))"
}

bundle_gopls() {
    echo "==> Bundling gopls ${GOPLS_VERSION}"

    if command -v go >/dev/null 2>&1; then
        echo "    Building with go install..."
        GOBIN="$MACOS_DIR" GO111MODULE=on go install "golang.org/x/tools/gopls@${GOPLS_VERSION}"
    else
        echo "WARNING: go not found — skipping gopls bundle (install Go on the build machine)"
        return 0
    fi

    if [ ! -x "$MACOS_DIR/gopls" ]; then
        echo "ERROR: gopls binary not found after install"
        exit 1
    fi
    echo "    Binary: $MACOS_DIR/gopls ($("$MACOS_DIR/gopls" version 2>&1 | head -1))"
}

echo "==> Bundling language servers for macOS ${ARCH}"
bundle_pyright
bundle_typescript_language_server
bundle_rust_analyzer
bundle_gopls

LSP_SIZE=$(du -sh "$LSP_DIR" 2>/dev/null | cut -f1 || echo "n/a")
echo "==> Language server bundle complete (Resources/lsp: ${LSP_SIZE})"
