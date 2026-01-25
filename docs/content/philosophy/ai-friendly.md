+++
title = "Why AI-Friendly"
description = "Predictable structure for humans and machines"
weight = 3
+++

## AI Is Changing Development

Large language models are becoming coding assistants. They can generate code, explain bugs, suggest improvements.

But they work best with predictable patterns. Ambiguity confuses them just like it confuses humans.

## What Makes Code AI-Friendly

### 1. Consistent Patterns

Every Rapina handler looks the same:

```rust
#[get("/path")]
async fn handler_name(extractors...) -> Result<Response> {
    // logic
}
```

An AI can understand and generate this pattern reliably.

### 2. Clear Conventions

- Routes are `GET /resource/:id`
- Errors are `Error::not_found("message")`
- Config is `#[derive(Config)]`

No guessing. No "it depends".

### 3. Self-Documenting Code

Types tell the story:

```rust
async fn get_user(
    id: Path<u64>,           // URL param, must be u64
    user: CurrentUser,        // Requires auth
) -> Result<Json<User>>      // Returns JSON or error
```

An AI (or human) can understand this without reading the implementation.

### 4. Explicit Over Implicit

Nothing hidden:
- Auth is visible (`CurrentUser` extractor)
- Public routes are marked (`#[public]`)
- Errors are typed (`Result<T, Error>`)

## The Benefit

When you use AI tools with Rapina:

1. **Generation works better.** Clear patterns = accurate suggestions.
2. **Explanations are clearer.** Consistent structure = better analysis.
3. **Refactoring is safer.** Predictable code = confident changes.

## Human-Friendly Too

What's good for AI is good for humans:
- New developers onboard faster
- Code reviews are easier
- Maintenance is simpler

Rapina's structure benefits everyone who reads the code - human or machine.
