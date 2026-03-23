---
name: No fake issue/PR numbers
description: Never reference issue or PR numbers in commit messages unless they actually exist
type: feedback
---

Never fabricate issue or PR numbers (e.g. `#350`) in commit messages. Only reference an issue/PR number if it was provided by the user or confirmed to exist.

**Why:** User caught a made-up `#350` reference in a commit message — it linked to nothing and is misleading.

**How to apply:** When writing commit messages, omit issue/PR references unless the user explicitly provides one.
