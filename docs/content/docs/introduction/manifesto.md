+++
title = "Manifesto"
weight = 2
date = 2025-02-13
+++

Rapina is built around three core beliefs.

## Decisions should be made for you, not by you

Most APIs need the same things: JSON handling, authentication, validation, error formatting. Rapina makes these decisions upfront so you can ship faster.

When you need to diverge, escape hatches exist. But you shouldn't need them for 90% of use cases.

## If it compiles, it should work

The type system is your first line of defense. Extractors are typed, errors are typed, routes are checked at compile time.

Runtime surprises should be rare. When something can fail, the types tell you. When something is missing, the compiler tells you.

## Code should be readable by humans and machines alike

Predictable patterns make onboarding faster, code reviews easier, and AI assistance more effective.

A handler's signature tells you everything: what it expects, what it returns, whether it's protected. No magic, no hidden behavior.

## Opinionated by design

We believe that constraints breed creativity. By making decisions for you, we free you to focus on what matters: your business logic.

- Routes are protected by default
- Errors follow a standard format
- Validation happens automatically
- OpenAPI is generated from code

You can override any of these. But you probably won't need to.

## Fail fast, fail loud

Missing configuration? Crash at startup, not at 3am when a user hits that endpoint.

Invalid state? Return a typed error, not a generic 500.

Bad request? Tell the client exactly what's wrong with a 422 and structured validation errors.
