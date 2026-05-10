# Agent Notes

## PR And Main Branch Rules

- `main` is the protected branch for this repository.
- Do not push directly to `main`.
- Changes that need to enter `main` should be made on a separate branch and submitted as a pull request.
- Pull requests into `main` must pass the required GitHub Actions CI checks before merging.
- Required checks are:
  - `Build and test (windows-latest)`
  - `Build and test (ubuntu-22.04)`
  - `Build and test (macos-latest)`
- If a PR has conflicts, resolve them on the PR branch, then push the resolved branch so GitHub can rerun CI.
- Do not force-push or delete `main`.

## Local Agent Notes

- Git branch create/switch operations may need escalated permission first; request approval before retrying.
- If PowerShell displays UTF-8 source text as mojibake, verify with a UTF-8 reader before assuming the file is corrupted.
- Run `pnpm format:check` after TS/TSX/i18n edits and format before pushing.
- Before pushing a PR branch, run the CI-equivalent local checks from `.github/workflows/ci.yml` when feasible:
  - `pnpm lint`
  - `pnpm format:check`
  - `pnpm build`
  - `cargo fmt --all -- --check` from `src-tauri/`
  - `cargo clippy --all-targets --all-features --locked -- -D warnings` from `src-tauri/`
  - `cargo test --locked` from `src-tauri/`
