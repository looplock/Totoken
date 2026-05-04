# Contributing

Thanks for helping improve Totoken. This guide keeps contributions predictable and easy to review.

## Reporting Bugs

Before opening a bug report, search existing issues to avoid duplicates:

https://github.com/looplock/Totoken/issues

When you open a bug report, include:

- Totoken version or commit SHA.
- Operating system and version.
- The source app involved, if relevant, such as Claude Code, Codex, Cursor, OpenCode, Kilo Code, or Kiro.
- Clear steps to reproduce the problem.
- What you expected to happen and what actually happened.
- Screenshots, logs, or error messages when they help.

Do not attach private prompts, messages, code, API keys, or local database files unless you have removed sensitive data.

## Suggesting Changes

For feature requests or behavior changes, open an issue first when the change is large, user-visible, or affects storage, scanning, pricing, privacy, release packaging, or supported source apps.

Small documentation fixes, typo fixes, and focused bug fixes can go straight to a pull request.

## Development Setup

Install the project prerequisites:

- Node.js 22+
- pnpm 10+
- Rust stable toolchain
- Tauri 2 prerequisites for your operating system

Install dependencies:

```bash
pnpm install
```

Run the frontend development server:

```bash
pnpm dev
```

Run the Tauri desktop app in development mode:

```bash
pnpm tauri:dev
```

## Pull Requests

Before opening a pull request:

- Keep the change focused on one issue or one coherent improvement.
- Update documentation when behavior, setup, commands, or user-visible text changes.
- Add or update checks when the change affects shared logic, parsers, storage, or UI behavior.
- Avoid committing generated release artifacts or local application data.
- Link the related issue when one exists.

In the pull request description, include:

- What changed.
- Why the change is needed.
- How you tested it.
- Any known follow-up work or limitations.

## Quality Checks

Run the checks that match your change. For most code changes, run all frontend checks:

```bash
pnpm lint
pnpm format:check
pnpm test
pnpm build
```

For Rust backend changes, also run:

```bash
pnpm rust:fmt
pnpm rust:clippy
cd src-tauri
cargo test
```

If a check cannot be run locally, mention that in the pull request and explain why.

## Privacy Notes

Totoken works with local AI coding tool history. Contributions should preserve the project's local-first privacy model:

- Do not upload sessions, prompts, messages, code, local databases, or API credentials.
- Keep network access explicit and limited to documented features.
- Treat sample data, fixtures, and screenshots as public information.

## License

By contributing, you agree that your contribution will be licensed under the MIT License used by this project.
