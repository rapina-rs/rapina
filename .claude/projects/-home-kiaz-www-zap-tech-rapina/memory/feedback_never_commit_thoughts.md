---
name: Never commit the thoughts folder
description: The thoughts/ directory must never be committed or staged — it is private/local only
type: feedback
---

Never commit or stage anything from the `thoughts/` directory. It is local-only and must not be pushed to the repository.

**Why:** User was upset that a plan file from `thoughts/shared/plans/` was included in a commit and pushed.

**How to apply:** When staging files for commits, always exclude `thoughts/` entirely. Only commit source code and project files.
