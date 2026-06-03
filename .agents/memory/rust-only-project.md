---
name: Rust-only project
description: This Replit is a pure Rust CLI project — no Node.js, no frontend, no pnpm workspace.
---

This workspace is now a single-crate Rust project at the root.

**Why:** User explicitly wanted the web app scaffold removed and replaced with a pure Rust research CLI. All Node.js/TypeScript/React files were deleted.

**How to apply:**
- Do NOT create any Node.js, npm, or frontend artifacts here
- Do NOT use pnpm, package.json, or TypeScript
- The only workflow is `Build` (runs `cargo build`)
- Install packages with `installLanguagePackages({ language: "rust", packages: [...] })` if crates are ever needed
- The active module is `rust-stable`; nodejs-24 is uninstalled

**Crate structure:**
- Binary: `northflow` (src/main.rs)
- Library: `northflow_crypto_trading_bot` (src/lib.rs)
- All in one Cargo.toml at root (not a workspace — single crate with both bin and lib)
- Edition 2024, rust-version 1.85

**Config parsing:** Manual TOML line-by-line parser in src/config/mod.rs — no serde or toml crate. If the user wants to add the toml crate later, that's fine, but don't add it proactively.
