# ADR 0016: Stable error codes instead of message matching

Status: accepted
Date: 2026-11-23

## Context

A wording change in a core message would have made the UI show "file deleted" as an ordinary conflict.

## Decision

Errors carry a stable `ApiError.code` where the UI must react to a precise cause (`too_large`, `network_path`, `not_allowed`, `read_only`, `gone`). `message` is English text for humans and may change at any time. The UI never recognises an error by its `message`.

## Consequences

- New UI reactions need a new code in `app/CONTRACT.md` first.
- Test: `error_and_payload_json_shapes` in `app/crates/rocksy-core/tests/contract.rs`; UI: `recognises errors by their code` in `app/ui/tests/sync.test.ts`.

## Sources

Journal 2026-11-23 19:03; `app/CONTRACT.md` (Errors).
