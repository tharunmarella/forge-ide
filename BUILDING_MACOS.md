# Quick Start: Building & Distributing Forge IDE for macOS

## 🚀 For Quick Testing (Current Method)

Your friend needs to manually approve the app:

```bash
# Build the app
./scripts/package-macos.sh

# Send them: dist/Forge-IDE.app
```

**Tell your friend:**
"Right-click `Forge-IDE.app` and choose **Open** (don't double-click). Click **Open** in the dialog."

---

## ✅ For Proper Distribution (No Warnings)

### One-Time Setup

1. **Check if you have a signing certificate:**
   ```bash
   ./scripts/check-codesign.sh
   ```

2. **If you don't have one, get it:**
   - Sign up at https://developer.apple.com/programs/ ($99/year)
   - Create a "Developer ID Application" certificate
   - Follow instructions at: `CODESIGNING_GUIDE.md`

3. **Set your signing identity:**
   ```bash
   # Get your identity name
   security find-identity -v -p codesigning
   
   # Export it (or add to ~/.zshrc)
   export CODESIGN_IDENTITY="Developer ID Application: Your Name (TEAM12345)"
   ```

### Build Signed App

```bash
# Build with proper signing
./scripts/package-macos.sh

# Result: dist/Forge-IDE.app (properly signed)
```

Users can now open it without any warnings! 🎉

---

## 🔐 For Professional Distribution (Best Experience)

After signing, also notarize:

```bash
# 1. Build signed app
export CODESIGN_IDENTITY="Developer ID Application: Your Name (TEAM_ID)"
./scripts/package-macos.sh

# 2. Create zip
cd dist
ditto -c -k --keepParent Forge-IDE.app Forge-IDE.zip

# 3. Submit for notarization (need app-specific password from appleid.apple.com)
xcrun notarytool submit Forge-IDE.zip \
  --apple-id your@email.com \
  --team-id TEAM12345 \
  --password abcd-efgh-ijkl-mnop \
  --wait

# 4. Staple the ticket
xcrun stapler staple Forge-IDE.app

# 5. Verify
spctl --assess --verbose=4 --type execute Forge-IDE.app
```

---

## 📦 Creating a DMG

```bash
# Build with DMG
./scripts/package-macos.sh --dmg

# Result: dist/Forge-IDE-0.4.6-arm64.dmg
```

---

## 🆘 Troubleshooting

### Users see "damaged and can't be opened"
```bash
# Remove quarantine attribute (for testing only)
xattr -dr com.apple.quarantine Forge-IDE.app
```

### Check current signature status
```bash
codesign --display --verbose=4 dist/Forge-IDE.app
```

### Remove signature and re-sign
```bash
codesign --remove-signature dist/Forge-IDE.app
./scripts/package-macos.sh --skip-build
```

---

## 📚 More Info

- Full guide: `CODESIGNING_GUIDE.md`
- Check setup: `./scripts/check-codesign.sh`
- Apple docs: https://developer.apple.com/support/code-signing/
