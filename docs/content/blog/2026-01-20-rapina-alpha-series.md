+++
title = "Rapina v0.1.0 Alpha Series"
description = "A look back at the four alpha releases that established the foundation of the Rapina framework"
date = 2026-01-20

[taxonomies]
categories = ["release-notes"]
tags = ["release", "alpha"]

[extra]
author = "uemuradevexe"
+++

Between January 20 and 23, 2026, we shipped four alpha releases (**v0.1.0-alpha.1** through **v0.1.0-alpha.4**) that together laid the foundation for the Rapina framework. Here's everything that landed.

## Router & Extractors

The core router shipped with support for **path parameters**, letting you define expressive routes out of the box.

Alongside the router, Rapina introduced a full set of typed extractors that pull data from incoming requests with zero boilerplate: `Json<T>`, `Path<T>`, `Query<T>`, `Form<T>`, `Headers`, and `State<T>`.

The `Validated<T>` extractor also shipped, giving you structured validation errors automatically when input doesn't meet your constraints.

## CLI

Two commands made it into the alpha series: `rapina new` scaffolds a new project with a sensible default structure, and `rapina dev` starts a development server with live reload.

## Error Handling

Error handling was designed as a first-class concern from day one. All errors carry a `trace_id`, making it straightforward to correlate requests across logs. Domain errors were treated as first-class citizens. You can define your own error types and have them serialized consistently through the standard error pipeline.

## Middleware

Three built-in middlewares shipped during the alpha series: `Timeout` aborts requests that exceed a configured duration, `BodyLimit` rejects request bodies above a size threshold, and `TraceId` attaches a unique trace identifier to every request.

## OpenAPI

Rapina generated **OpenAPI 3.0 specs automatically** from your route and extractor definitions, with no annotations required. We also shipped `rapina openapi diff`, a CLI command that detects breaking changes between two spec versions, making it easier to evolve your API safely.

## Testing

A dedicated **test client** shipped for integration testing, letting you send requests to your application in tests without spinning up a real server.

## Observability

Structured logging and tracing were wired in from the start. A built-in route introspection endpoint at `/__rapina/routes` returned a list of all registered routes at runtime, useful for debugging and tooling.

## Release Timeline

| Version        | Date         |
|----------------|--------------|
| v0.1.0-alpha.1 | January 20   |
| v0.1.0-alpha.2 | January 21   |
| v0.1.0-alpha.3 | January 21   |
| v0.1.0-alpha.4 | January 23   |

Thanks to everyone who tried the alphas, opened issues, and gave feedback. The foundation is set. More to come.
