---
title: "ADR-0027: JVM and Android share one generated JNI layer"
status: proposed
updated: 2026-11-06
---

# ADR-0027: JVM and Android share one generated JNI layer

Status: proposed, 2026-11-06

## Context

Android has no Java FFM, and many JVM users are on Java 17 or 21, where FFM is not final.

## Decision

One generated JNI layer (Rust side through the `jni` crate, Java/Kotlin side generated) serves both the Android AAR and the desktop JAR on Maven Central under `com.teispace`. An FFM backend may come later if users ask. Android libraries are built with cargo-ndk and must meet Google Play's 16 KB page-size rule, which is checked in CI before the first AAR (Play blocks non-compliant updates from 2027-02-01).

## Consequences

- A 16 KB ELF alignment gate.
- One Kotlin-friendly API for both targets.

## Alternatives considered

FFM for the JVM plus JNI for Android (two backends to maintain); JNA (slow, reflective).

## Evidence

[Bindings and MCP](../../02-research/notes/bindings-and-mcp.md), 2026-11-06.
