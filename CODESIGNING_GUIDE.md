# macOS Code Signing Guide for Forge IDE

## The Problem

When users download and try to open Forge IDE on macOS, they see:

> "Forge IDE.app" cannot be opened because the developer cannot be verified.

They must then go to **System Preferences → Privacy & Security** and click "Open Anyway" to use the app.

**Why?** The app is currently signed with an **ad-hoc signature** (codesign with `-` flag), which macOS treats as untrusted.

---

## Solutions (Choose One)

### ✅ Option 1: Proper Code Signing (Recommended)

**What you need:**
- Apple Developer Account ($99/year)
- Developer ID Application certificate

**Steps:**

#### 1. Get an Apple Developer Account
- Go to https://developer.apple.com/programs/
- Enroll for $99/year

#### 2. Create a Developer ID Certificate
1. Go to https://developer.apple.com/account/resources/certificates/list
2. Click the **+** button
3. Select **Developer ID Application** (for apps distributed outside the Mac App Store)
4. Follow the instructions to create a Certificate Signing Request (CSR)
5. Download and install the certificate in your Keychain

#### 3. Find Your Signing Identity
```bash
security find-identity -v -p codesigning
```

You'll see output like:
```
1) ABC123DEF456 "Developer ID Application: Your Name (TEAM12345)"
```

#### 4. Build and Sign
```bash
export CODESIGN_IDENTITY="Developer ID Application: Your Name (TEAM12345)"
./scripts/package-macos.sh
```

**Result:** Users can open the app without any warnings! ✅

---

### ⚠️ Option 2: Ad-hoc Signing (Current Method)

**What it does:**
- Allows the app to run on ARM64 Macs (required)
- BUT users must manually approve it in Security settings

**To use:**
```bash
./scripts/package-macos.sh
```

**User experience:**
1. Download Forge-IDE.app
2. Try to open → blocked
3. Go to System Preferences → Privacy & Security
4. Click "Open Anyway"
5. Confirm again

---

### 🔐 Option 3: Full Notarization (Best for Public Distribution)

After code signing, you can also **notarize** the app with Apple. This removes ALL warnings.

**Steps:**

#### 1. Sign the app (see Option 1)

#### 2. Create a zip
```bash
cd dist
ditto -c -k --keepParent Forge-IDE.app Forge-IDE.zip
```

#### 3. Create an app-specific password
1. Go to https://appleid.apple.com/account/manage
2. Sign in
3. Go to **Security** → **App-Specific Passwords**
4. Click **Generate Password**
5. Copy the password (looks like: `abcd-efgh-ijkl-mnop`)

#### 4. Submit for notarization
```bash
xcrun notarytool submit Forge-IDE.zip \
  --apple-id YOUR_EMAIL@example.com \
  --team-id TEAM12345 \
  --password abcd-efgh-ijkl-mnop \
  --wait
```

This will take 5-15 minutes. You'll see:
```
  status: Accepted
```

#### 5. Staple the notarization ticket
```bash
xcrun stapler staple dist/Forge-IDE.app
```

#### 6. Verify
```bash
spctl --assess --verbose=4 --type execute dist/Forge-IDE.app
```

Should output:
```
dist/Forge-IDE.app: accepted
source=Notarized Developer ID
```

**Result:** Users can download and open immediately with ZERO warnings! 🎉

---

## Quick Reference

### Current Script Behavior

The updated `scripts/package-macos.sh` now:
- ✅ Checks for `CODESIGN_IDENTITY` environment variable
- ✅ Uses proper signing if set (with `--options runtime` for notarization)
- ✅ Falls back to ad-hoc signing if not set (with helpful warnings)
- ✅ Provides instructions on how to sign properly

### Environment Variables

```bash
# For proper signing
export CODESIGN_IDENTITY="Developer ID Application: Your Name (TEAM12345)"

# For notarization (optional)
export APPLE_ID="your_email@example.com"
export TEAM_ID="TEAM12345"
export NOTARY_PASSWORD="abcd-efgh-ijkl-mnop"
```

### Commands Cheat Sheet

```bash
# Find your signing identity
security find-identity -v -p codesigning

# Build with proper signing
export CODESIGN_IDENTITY="Developer ID Application: Your Name (TEAM_ID)"
./scripts/package-macos.sh

# Verify signature
codesign --verify --deep --strict --verbose=2 dist/Forge-IDE.app

# Check what macOS thinks of the app
spctl --assess --verbose=4 --type execute dist/Forge-IDE.app

# Test Gatekeeper (simulate first launch)
xattr -d com.apple.quarantine dist/Forge-IDE.app
```

---

## For Your Friend (Current Ad-hoc Signed Build)

If you send them the current build, tell them:

1. Download `Forge-IDE.app`
2. When you try to open it, macOS will block it
3. Go to **System Preferences** → **Privacy & Security** (or **System Settings** on newer macOS)
4. Scroll down and click **"Open Anyway"** next to the Forge IDE message
5. Click **"Open"** in the confirmation dialog

**Alternatively**, they can right-click the app and choose **"Open"** (this is faster).

---

## Recommended Approach

1. **For testing/friends:** Continue with ad-hoc signing (current method)
   - Free
   - Works fine, just requires one manual approval

2. **For public distribution:** Get Developer ID and sign properly
   - $99/year
   - Users can open immediately

3. **For professional distribution:** Sign + Notarize
   - Best user experience
   - Zero friction
   - Required for some enterprise environments

---

## Troubleshooting

### "invalid signature" error
```bash
# Remove old signature and re-sign
codesign --remove-signature dist/Forge-IDE.app
./scripts/package-macos.sh
```

### "the signature does not include a secure timestamp"
- Add `--timestamp` flag (already in updated script)

### Notarization fails with "invalid" status
```bash
# Get detailed log
xcrun notarytool log <submission_id> --apple-id YOUR_EMAIL --team-id TEAM_ID --password PASSWORD
```

Common issues:
- Binary not signed with hardened runtime (`--options runtime`)
- Using ad-hoc signature instead of Developer ID
- Binary includes restricted entitlements

---

## Resources

- [Apple Code Signing Guide](https://developer.apple.com/support/code-signing/)
- [Notarization Guide](https://developer.apple.com/documentation/security/notarizing_macos_software_before_distribution)
- [Developer ID Certificate](https://developer.apple.com/developer-id/)

---

## Summary

**Current state:** Ad-hoc signed → requires manual approval  
**To fix:** Sign with Developer ID → no warnings  
**Best experience:** Sign + Notarize → zero friction

The script has been updated to support both methods via the `CODESIGN_IDENTITY` environment variable.
