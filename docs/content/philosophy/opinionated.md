+++
title = "Why Opinionated"
description = "Convention over configuration"
weight = 1
+++

## The Problem with Flexibility

Most frameworks pride themselves on flexibility. "Use any ORM!" "Choose your own validation library!" "Structure your project however you want!"

This sounds great until you realize:

1. **Every decision is a tax on productivity.** Each choice requires research, evaluation, and maintenance.

2. **Teams fragment.** Without conventions, each developer creates their own patterns. Codebases become inconsistent.

3. **AI struggles.** Large language models work best with predictable patterns. Flexibility creates ambiguity.

## 90% of Apps Need 10% of Decisions

Most APIs need the same things:
- JSON request/response handling
- Path and query parameters
- Authentication (usually JWT)
- Validation
- Error handling
- OpenAPI documentation

Rapina makes these decisions for you. The result is less code, fewer bugs, and faster development.

## The Escape Hatch

Opinionated doesn't mean inflexible. Rapina provides escape hatches when you need them:

- Custom extractors for special cases
- Middleware for cross-cutting concerns
- Direct access to `hyper` types when needed

But the default path should work for 90% of use cases without any configuration.

## What This Means in Practice

| Decision | Rapina's Choice |
|----------|-----------------|
| Serialization | JSON with Serde |
| Authentication | JWT Bearer tokens |
| Route protection | Protected by default |
| Error format | Standardized envelope with trace_id |
| Documentation | OpenAPI 3.0 |
| Configuration | Environment variables |

You can override these, but you probably won't need to.
