---
name: Always run cargo fmt, test, and clippy
description: User wants cargo fmt, test, and clippy run as verification steps, not just test and clippy
type: feedback
---

Always run `cargo fmt`, `cargo test`, and `cargo clippy` as verification steps in plans and after implementation.

**Why:** User explicitly corrected a plan that only listed test and clippy — they want fmt included too.

**How to apply:** Include `cargo fmt` in all Rust plan success criteria and run it as part of verification after writing code.
