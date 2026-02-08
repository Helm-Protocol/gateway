#!/usr/bin/env bash
# ============================================
# Anonymity Pre-Push Check
# Run this before pushing to verify no PII leaks
# ============================================
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

ERRORS=0

echo "=== Anonymity Check ==="
echo ""

# 1. Verify git identity
echo -n "[1/6] Git identity... "
GIT_NAME=$(git config user.name)
GIT_EMAIL=$(git config user.email)
if [[ "$GIT_NAME" == "Helm Protocol" ]]; then
    echo -e "${GREEN}OK${NC} ($GIT_NAME)"
else
    echo -e "${RED}FAIL${NC} — Git user.name is '$GIT_NAME', expected project pseudonym"
    echo "  Fix: git config user.name 'Helm Protocol'"
    ERRORS=$((ERRORS + 1))
fi

# 2. Check for personal email patterns in staged files
echo -n "[2/6] Staged files for email patterns... "
if git diff --cached --name-only | xargs grep -lE '[a-zA-Z0-9._%+-]+@(gmail|yahoo|hotmail|outlook|proton|icloud)\.[a-z]{2,}' 2>/dev/null; then
    echo -e "${RED}FAIL${NC} — Personal email found in staged files"
    ERRORS=$((ERRORS + 1))
else
    echo -e "${GREEN}OK${NC}"
fi

# 3. Check for common PII patterns (names, signed-off-by)
echo -n "[3/6] PII patterns in staged files... "
if git diff --cached | grep -iE '(Signed-off-by|Co-authored-by|@author|Author:)' 2>/dev/null; then
    echo -e "${RED}FAIL${NC} — Attribution header found"
    ERRORS=$((ERRORS + 1))
else
    echo -e "${GREEN}OK${NC}"
fi

# 4. Check for secrets/tokens
echo -n "[4/6] Secrets & tokens... "
if git diff --cached | grep -iE '(ghp_|github_pat_|gho_|sk-|AKIA|password\s*=|secret\s*=|token\s*=)' 2>/dev/null; then
    echo -e "${RED}FAIL${NC} — Possible secret/token detected"
    ERRORS=$((ERRORS + 1))
else
    echo -e "${GREEN}OK${NC}"
fi

# 5. Check for filesystem paths that reveal username
echo -n "[5/6] Filesystem path leaks... "
if git diff --cached | grep -E '(/Users/[a-zA-Z]|/home/[a-zA-Z]|C:\\\\Users\\\\[a-zA-Z])' 2>/dev/null; then
    echo -e "${YELLOW}WARN${NC} — Local filesystem path detected (may reveal username)"
    ERRORS=$((ERRORS + 1))
else
    echo -e "${GREEN}OK${NC}"
fi

# 6. Check for EXIF data in staged images
echo -n "[6/6] Image EXIF metadata... "
HAS_IMAGES=false
for img in $(git diff --cached --name-only | grep -iE '\.(png|jpg|jpeg|gif|tiff)$' 2>/dev/null); do
    HAS_IMAGES=true
    if command -v exiftool &> /dev/null; then
        EXIF_COUNT=$(exiftool -q -q "$img" 2>/dev/null | wc -l)
        if [ "$EXIF_COUNT" -gt 5 ]; then
            echo -e "${RED}FAIL${NC} — $img has EXIF metadata ($EXIF_COUNT tags)"
            echo "  Fix: exiftool -all= $img"
            ERRORS=$((ERRORS + 1))
        fi
    fi
done
if [ "$HAS_IMAGES" = false ]; then
    echo -e "${GREEN}OK${NC} (no images staged)"
else
    echo -e "${GREEN}OK${NC}"
fi

echo ""
if [ "$ERRORS" -gt 0 ]; then
    echo -e "${RED}=== $ERRORS issue(s) found — fix before pushing ===${NC}"
    exit 1
else
    echo -e "${GREEN}=== All checks passed — safe to push ===${NC}"
    exit 0
fi
