---
title: "ADR-0005: teigeodata stands alone"
status: accepted
updated: 2026-11-06
---

# ADR-0005: teigeodata stands alone

Status: accepted, 2026-11-06

## Context

Other Teispace projects (for example an astrology SDK) need places and timezones. Coupling to them would slow both.

## Decision

teigeodata has **no dependency on any other Teispace project**, shares no crates with them, and is designed as the best general-purpose geodata SDK. Other projects adopt it later as ordinary consumers. Patterns proven elsewhere may be re-implemented here after research.

## Consequences

- Our own IDL and binding generators ([ADR-0016](ADR-0016-bindings.md)).

## Alternatives considered

Sharing an IDL crate with another SDK (rejected 2026-11-06 to avoid coupling).

## Evidence

Maintainer decision, 2026-11-06.
