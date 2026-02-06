# Helm Project — Claude Code Settings

## STRICT ANONYMITY POLICY

This project operates under absolute anonymity. All contributors are pseudonymous.
The original creator's identity must never be discoverable through any artifact in this repository.

### Identity Rules
- **Git identity**: Always use `Helm Protocol <noreply@helm-protocol.org>`
- Before EVERY commit, verify: `git config user.name && git config user.email`
- **NEVER** use real names, personal emails, employer names, or any PII
- **NEVER** add `Signed-off-by`, `Co-authored-by`, `@author`, or attribution headers

### Code Hygiene
- No personal identifiers in code, comments, or documentation
- No timezone-revealing timestamps in manual entries
- No locale-specific strings that reveal geography (use English only in code)
- No references to personal GitHub accounts, social media, or external profiles
- No writing-style fingerprints — use neutral, technical language
- No IDE-specific settings with local filesystem paths

### Commit Rules
- Use conventional commits: `feat:`, `fix:`, `refactor:`, `docs:`, `chore:`, `test:`
- Keep messages purely technical — describe WHAT changed, not WHO did it
- No `Signed-off-by` or `Reviewed-by` trailers
- Run `scripts/anonymity-check.sh` before pushing if available

### Metadata & Files
- Strip EXIF from all images before committing
- Never commit `.env`, tokens, API keys, or credentials
- Use `.gitattributes` to strip binary metadata
- All text files must use LF line endings (prevents OS fingerprinting)

### Secrets
- Use environment variables for all sensitive config
- NEVER paste tokens in conversations — use OAuth device flow or env vars
- Credential files are gitignored

### Handoff Protocol
When delegating this project to new contributors:
1. They must configure their local git with the project pseudonym
2. Share this CLAUDE.md as the onboarding document
3. Never reveal the original creator's identity
4. All communication about this project should use the pseudonym only
