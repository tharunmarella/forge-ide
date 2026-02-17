#!/bin/bash
# Quick setup script for code signing Forge IDE on macOS

echo "🔐 Forge IDE Code Signing Setup"
echo "================================"
echo ""

# Check if we have a Developer ID certificate
echo "Checking for available signing identities..."
IDENTITIES=$(security find-identity -v -p codesigning 2>/dev/null | grep "Developer ID Application")

if [ -z "$IDENTITIES" ]; then
    echo ""
    echo "❌ No Developer ID Application certificate found"
    echo ""
    echo "You have two options:"
    echo ""
    echo "1️⃣  Use ad-hoc signing (current method)"
    echo "   - Free"
    echo "   - Users must approve in Security settings on first launch"
    echo "   - Command: ./scripts/package-macos.sh"
    echo ""
    echo "2️⃣  Get a Developer ID certificate"
    echo "   - \$99/year Apple Developer account"
    echo "   - No warnings for users"
    echo "   - See CODESIGNING_GUIDE.md for full instructions"
    echo ""
    exit 0
fi

echo ""
echo "✅ Found Developer ID certificate(s):"
echo "$IDENTITIES"
echo ""

# Extract the identity
IDENTITY=$(echo "$IDENTITIES" | head -1 | sed -E 's/.*"(.*)"/\1/')

echo "To build with proper code signing, run:"
echo ""
echo "  export CODESIGN_IDENTITY=\"$IDENTITY\""
echo "  ./scripts/package-macos.sh"
echo ""
echo "Or add to your ~/.zshrc or ~/.bash_profile:"
echo ""
echo "  export CODESIGN_IDENTITY=\"$IDENTITY\""
echo ""
echo "For more information, see CODESIGNING_GUIDE.md"
echo ""
