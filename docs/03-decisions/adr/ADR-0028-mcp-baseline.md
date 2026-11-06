---
title: "ADR-0028: MCP baseline: revision 2026-07-28 on rmcp"
status: accepted
updated: 2026-11-06
---

# ADR-0028: MCP baseline: revision 2026-07-28 on rmcp

Status: accepted, 2026-11-06

## Context

MCP from day 0 ([ADR-0017](ADR-0017-mcp.md)) needs a concrete protocol baseline and implementation.

## Decision

Target MCP revision **2026-07-28** (stateless lifecycle, `server/discover`, sessionless Streamable HTTP), with fallback to **2025-11-25** for older clients. Build on the official Rust SDK `rmcp` (3.9.0 checked 2026-11-06). Ship stdio first, then stateless Streamable HTTP. Every tool has an `outputSchema` and returns `structuredContent` plus a short text summary; results are size-bounded with cursors; datasets and places are also exposed as resources.

## Consequences

- An MCP conformance gate for both revisions.
- Deprecated features (sampling, roots, logging) are not used.

## Alternatives considered

Our own protocol implementation (more work, no benefit while `rmcp` tracks the spec).

## Evidence

[Bindings and MCP](../../02-research/notes/bindings-and-mcp.md), 2026-11-06.
