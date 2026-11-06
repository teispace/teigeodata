---
title: "ADR-0017: MCP server from day 0"
status: accepted
updated: 2026-11-06
---

# ADR-0017: MCP server from day 0

Status: accepted, 2026-11-06

## Context

AI agents are a primary consumer of place data.

## Decision

`teigeodata-mcp` speaks the latest Model Context Protocol revision (and the previous one for older clients), over stdio and Streamable HTTP, with a transport-independent message handler. It exposes tools (search, reverse, nearby, get, hierarchy, children, boundary, timezone, elevation, postal, distance, containment, packs, coverage), resources and prompts. It ships self-hosted; a hosted endpoint can come later ([ADR-0025](ADR-0025-hosted-ready.md)).

## Consequences

- MCP tools are covered by the parity gate's golden queries.

## Alternatives considered

An MCP server later (rejected: day 0).

## Evidence

Maintainer direction, 2026-11-06; [bindings and MCP](../../02-research/notes/bindings-and-mcp.md).
