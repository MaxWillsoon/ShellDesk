# Testing

ShellDesk uses layered tests so connection, backend, and UI regressions can be caught without requiring a real SSH host for every check.

## Default Gate

```bash
pnpm test
```

The default gate runs:

- IPC, desktop app, i18n, runtime-boundary, Tauri, default-settings, and release-script contract checks.
- `pnpm build` for TypeScript and Vite production build coverage.
- `pnpm check:ui` for mocked Playwright UI smoke tests.
- `pnpm check:rust` for Rust fmt, clippy with `-D warnings`, and Rust tests.
- `cargo check --manifest-path src-tauri/Cargo.toml`.

Run only the fast repository contract checks with:

```bash
pnpm check:contracts
```

## UI Smoke Tests

```bash
pnpm check:ui
```

Playwright serves `tests/ui/database-error-harness.html` through Vite and renders real React remote-desktop components with a mocked `window.guiSSH` bridge. The first covered flows are:

- MySQL create-table backend failure stays visible inside the create-table modal.
- Redis destructive action failure stays visible inside the confirmation modal.

These tests assert both DOM placement and z-index safety by checking that the alert is inside the active dialog and that `document.elementFromPoint()` at the alert center resolves back to the alert.

Install the browser once on fresh machines or CI workers:

```bash
pnpm exec playwright install chromium
```

## Rust Checks

```bash
pnpm check:rust
```

This runs:

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`

Rust tests include shared fixtures from `src-tauri/src/test_helpers.rs`, async database tunnel contract coverage, IPC database channel classification, and HTTP tunnel parameter/timeout validation.

## Coverage

```bash
cargo install cargo-llvm-cov
pnpm check:rust:coverage
```

CI installs `cargo-llvm-cov` and runs the coverage summary after the default test gate. The local command intentionally fails with an install hint if the tool is missing.

## Optional Live Smoke

```bash
pnpm smoke:ssh-live
```

The live SSH/SFTP smoke reads local `.env` or matching process environment variables:

- `SHELLDESK_TEST_SSH_HOST`
- `SHELLDESK_TEST_SSH_PORT`
- `SHELLDESK_TEST_SSH_USERNAME`
- `SHELLDESK_TEST_SSH_PASSWORD`

Do not commit real credentials. The smoke is intentionally separate from `pnpm test` because it requires an external server.
