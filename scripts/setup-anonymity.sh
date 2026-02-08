#!/usr/bin/env bash
# ============================================
# Anonymity Setup Script
# Run this after cloning to configure identity
# and install git hooks
# ============================================
set -euo pipefail

echo "=== Helm Project — Anonymity Setup ==="
echo ""

# Set pseudonymous git identity (local to this repo only)
git config user.name "Helm Protocol"
git config commit.gpgsign false
git config log.showSignature false

echo "[+] Git identity set to: Helm Protocol"

# Install pre-push hook
HOOK_DIR="$(git rev-parse --show-toplevel)/.git/hooks"
SCRIPT_DIR="$(git rev-parse --show-toplevel)/scripts"

cp "$SCRIPT_DIR/anonymity-check.sh" "$HOOK_DIR/pre-push"
chmod +x "$HOOK_DIR/pre-push"
echo "[+] Pre-push anonymity check hook installed"

echo ""
echo "=== Setup complete. All commits will use the project pseudonym. ==="
echo "    Verify with: git config user.name"
