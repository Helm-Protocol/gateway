#!/usr/bin/env bash
# ============================================
# Helm GCP Bootstrap
# Run on a fresh GCP instance to set up
# the Helm development and deployment environment
# ============================================
set -euo pipefail

echo "=== Helm Protocol — GCP Bootstrap ==="
echo ""

# 1. Git identity
git config --global user.name "Helm Protocol"
git config --global commit.gpgsign false
echo "[+] Git identity configured"

# 2. Install Rust
if ! command -v cargo &> /dev/null; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
    echo "[+] Rust installed"
else
    echo "[=] Rust already installed ($(rustc --version))"
fi

# 3. Install system dependencies
if command -v apt-get &> /dev/null; then
    sudo apt-get update -qq
    sudo apt-get install -y -qq build-essential pkg-config libssl-dev protobuf-compiler tor
    echo "[+] System dependencies installed"
fi

# 4. Configure Tor
if command -v tor &> /dev/null; then
    sudo systemctl enable tor
    sudo systemctl start tor
    echo "[+] Tor service started"
fi

# 5. Clone repo if not already present
REPO_DIR="${HELM_DIR:-$HOME/Helm}"
if [ ! -d "$REPO_DIR/.git" ]; then
    git clone https://github.com/Helm-Protocol/Helm.git "$REPO_DIR"
    echo "[+] Helm repo cloned to $REPO_DIR"
else
    cd "$REPO_DIR" && git pull origin main
    echo "[=] Helm repo updated"
fi

# 6. Run anonymity setup
cd "$REPO_DIR"
bash scripts/setup-anonymity.sh

# 7. Build
cargo build --release 2>/dev/null && echo "[+] Build successful" || echo "[!] Build skipped (no src yet)"

echo ""
echo "=== Bootstrap complete ==="
echo "    Repo: $REPO_DIR"
echo "    Next: cd $REPO_DIR && cargo run"
