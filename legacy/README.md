# Legacy Reference

This directory contains read-only reference code from previous Northflow / ARIA iterations.

## Contents

| Path | Description |
|------|-------------|
| `aria/` | Previous ARIA / crypto-scalper codebase (cloned from the user's earlier repo) |

## Rules

- **Legacy code is reference-only.** Never import from `legacy/` into the active `src/` tree.
- Before reusing any legacy logic, validate it against the Phase 1 checklist:
  1. Is it relevant to the current phase?
  2. Is it deterministic?
  3. Does it avoid LLM, agents, Telegram, dashboard, and live exchange side effects?
  4. Can it be simplified, tested, and aligned with the new Northflow architecture?
- If useful logic is identified, migrate it manually into the appropriate `src/` module with its own unit tests.

## Status

Active core has been rebuilt from scratch in `src/core/` as of Phase 1.
The legacy tree is preserved here for attribution and reference only.
