+++
title = "Tutorial"
description = "Learn Rapina by building — interactive exercises that teach you the framework step by step."
sort_by = "weight"
template = "section.html"
+++

# Interactive Tutorial

Learn Rapina by writing code. Each chapter gives you a task, an editor, and instant feedback — no compilation needed.

The tutorial covers the core framework concepts progressively. Each chapter builds on the previous one, growing your mental model from a simple handler all the way to database queries.

## Chapters

1. [Your First Route](/tutorial/01-your-first-route/) — handler basics, route macros, `#[public]`, returning JSON
2. [Protected Routes](/tutorial/02-protected-routes/) — authentication, `CurrentUser` extractor, 401 responses
3. [Validation](/tutorial/03-validation/) — `Validated<Json<T>>`, derive macros, 422 error envelopes
4. [Error Handling](/tutorial/04-error-handling/) — custom error types, `IntoApiError`, trace IDs
5. [State and Config](/tutorial/05-state-and-config/) — `#[derive(Config)]`, `State<T>`, environment variables
6. [Database Basics](/tutorial/06-database-basics/) — `Db` extractor, `schema!` macro, queries

Each exercise has tests that validate your code as you type. When all tests pass, you'll see the simulated HTTP response your handler would produce.
