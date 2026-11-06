---
title: "ADR-0025: Ready for a hosted tier, without one now"
status: accepted
updated: 2026-11-06
---

# ADR-0025: Ready for a hosted tier, without one now

Status: accepted, 2026-11-06

## Context

No hosted API or MCP now, but users may later want a paid hosted tier with the same API and all data.

## Decision

Auth, rate limits, quotas, metering and tenant routing live outside the core, behind traits in the server crate. The HTTP and MCP servers serve any pack set. The download manager treats any mirror alike. A hosted tier is a deployment, not a fork.

## Consequences

- No hosted code paths in the runtime crates.

## Alternatives considered

Building hosting now (out of scope), or ignoring it (a costly redesign later).

## Evidence

Maintainer direction, 2026-11-06.
