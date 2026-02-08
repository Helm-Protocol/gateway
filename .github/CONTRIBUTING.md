# Contributing to Helm

## Anonymity First

This project operates under strict pseudonymous contribution. Before contributing:

### Setup

```bash
# 1. Configure your git identity (REQUIRED)
git config user.name "Helm Protocol"
git config commit.gpgsign false

# 2. Verify your config
git config user.name   # Should show: Helm Protocol

# 3. Install the pre-push hook
cp scripts/anonymity-check.sh .git/hooks/pre-push
chmod +x .git/hooks/pre-push
```

### Rules

- All commits must use the project pseudonym
- No personal information in code, comments, or commit messages
- No `Signed-off-by` or `Co-authored-by` headers
- Run `scripts/anonymity-check.sh` before pushing
- Use conventional commit format: `feat:`, `fix:`, `refactor:`, etc.

### Handoff

If you are taking over development:
1. Read `CONTRIBUTING.md` thoroughly
2. Follow the setup steps above
3. Never attempt to identify previous contributors
4. Continue the work — the code speaks for itself
