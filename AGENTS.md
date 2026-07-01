# Agent Development Loop

This document defines the continuous improvement cycle for the BrowseDown project.

## The Loop

### 1. Complete Remaining TODO Items
Pick the next highest-priority item from `TODO.md` (or `ARCHITECTURE.md` if the task is architectural). Implement it with minimal, focused changes. Do not add speculative features.

### 2. Create Tests and Examples
For every new capability:
- Write integration tests in `tests/` that exercise the feature end-to-end where possible
- Add unit tests for core logic in `Browser`, `PageToMarkdown`, and other business-logic types
- Provide a minimal usage example if the feature is client-facing

### 3. Ensure `cargo build`, `cargo test`, and `cargo run` All Pass
After every change, **automatically run the verification commands** using the `run_command` tool in this order:

1. `cargo build` — compile must succeed with zero errors
2. `cargo test` — all unit tests must pass
3. `cargo run -- fetch <URL>` — app must launch and basic smoke test passes

**Do not ask the user to run these commands.** Execute them yourself as part of the workflow. If any step fails, fix the problem immediately before proceeding. Do not move on to the next TODO item while the build is broken. Warnings are acceptable but should be noted.

### 4. Loop Back to Step 1
Return to `TODO.md` and pick the next item. Repeat until the backlog is clear.

### 5. Audit and Optimize
After each batch of features, perform a quality pass:
- **Maintainability**: Are functions small and well-named? Is the module structure logical?
- **Leanness**: Remove dead code, unused imports, and speculative abstractions
- **Wiring**: Ensure all new features are properly integrated into `main.rs`, `Cargo.toml`, and docs
- **Small footprint**: Avoid unnecessary dependencies; prefer the standard library or lightweight crates
- **Consistency**: Match existing code style and patterns

### 6. Competitive Intelligence
Research similar headless browser and HTML-to-Markdown tools (html2text, readability-lxml, Mercury Parser, etc.). Identify capabilities they have that this project lacks. Add the most valuable ones to the `TODO.md` brainstorming section. Prioritize features that provide clear competitive advantage.

### 7. Update Documentation
Keep all project docs aligned with the current implementation:
- **`README.md`**: Quick start, feature list, architecture summary
- **`TODO.md`**: Mark completed items, move them to Done, keep brainstorming current
- **`SPEC.md`**: Scope and requirements, technical stack, quality bar
- **`ARCHITECTURE.md`**: Module relationships, data flow, deployment topology
- **`AGENTS.md`**: This file — update if the loop itself evolves

## Principles

- **Simplicity over flexibility**: Solve the problem at hand, not every hypothetical future problem
- **Surgical changes**: Touch only what you must; clean up only your own mess
- **Goal-driven**: Every change should have a verifiable success criterion
- **Test before ship**: No feature is complete until it has passing tests
- **Auto-verify**: Run `cargo build`, `cargo test`, and `cargo run` automatically after every change. Do not delegate verification to the user.
- **Docs are code**: Documentation drift is a bug
