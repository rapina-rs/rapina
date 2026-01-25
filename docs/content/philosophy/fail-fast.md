+++
title = "Why Fail Fast"
description = "Errors should be loud and early"
weight = 2
+++

## Silent Failures Are Dangerous

The worst bugs are the ones that don't crash. They silently corrupt data, return wrong results, or create security holes.

Rapina follows a simple principle: **if something is wrong, fail immediately and loudly.**

## Examples

### Configuration

When you start your app without required config:

```
Error: Missing environment variables: DATABASE_URL, JWT_SECRET
```

Not one at a time. All of them. Fix once, move on.

### Type Extraction

When a request doesn't match expected types:

```json
{
  "error": {
    "code": "BAD_REQUEST",
    "message": "invalid path param: expected integer, got 'abc'"
  }
}
```

The handler never runs with bad data.

### Authentication

When a route requires auth but no token is provided:

```json
{
  "error": {
    "code": "UNAUTHORIZED",
    "message": "missing authorization header"
  }
}
```

No "default user" or "anonymous access". Protected means protected.

## Compile-Time vs Runtime

Whenever possible, Rapina catches errors at compile time:

- Type mismatches in extractors
- Multiple body-consuming extractors
- Invalid route patterns

What can't be caught at compile time fails at startup or first use - not in production at 3am.

## Why This Matters

1. **Debugging is easier.** Errors point to the actual problem.
2. **Security is better.** No "fail open" behavior.
3. **Confidence is higher.** If it runs, it probably works.

## The Trade-Off

Fail-fast means your app won't start if something is wrong. This can be frustrating during development.

But it's a trade-off worth making. A few minutes of frustration during development beats hours of debugging in production.
