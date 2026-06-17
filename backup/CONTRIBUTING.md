# Contributing to ShellDesk

Thanks for taking the time to improve ShellDesk. Issues and pull requests in English or Chinese are welcome.

ShellDesk is an Electron 40 + React 19 + TypeScript + Vite desktop SSH client. It includes local vault storage, SSH connection management, SFTP, terminal sessions, remote desktop-style tools, database tools, VNC, sync, and packaging workflows.

## Before You Start

- Search existing issues and pull requests before opening a new one.
- Do not commit credentials, private keys, passphrases, API keys, tokens, real production hostnames, or private infrastructure details.
- Keep `.env` local. Use `.env.example` for placeholders only.
- For security vulnerabilities, follow [SECURITY.md](SECURITY.md) instead of opening a public issue.

## Development Setup

ShellDesk uses pnpm. The current expected package manager is `pnpm@10.26.2`.

```bash
pnpm install
pnpm dev
```

Useful commands:

```bash
pnpm typecheck
pnpm build
pnpm test
pnpm preview
pnpm release:dir
```

`pnpm test` currently runs the production build. For packaging-related changes, use the relevant `pnpm pack:*` or `pnpm release:*` command when practical.

If `pnpm dev` exits and Vite still occupies port `5173`, find the specific PID before stopping it:

```powershell
netstat -ano | findstr :5173
Stop-Process -Id <pid>
```

Do not kill all Node.js processes.

## Project Structure

Important areas:

- `electron/main.cjs` and `electron/main/*.cjs`: Electron main process, IPC handlers, SSH connection management, vault storage, remote command/SFTP/database/VNC logic.
- `electron/preload.cjs`: `contextBridge` API exposed to the renderer as `window.guiSSH`.
- `src/App.tsx`: main app shell, host/key/log/settings pages, vault sync, connection entry points.
- `src/RemoteDesktopShell.tsx`: remote desktop window system, app registry, Dock, Launchpad, layout persistence.
- `src/components/remote-desktop/`: remote desktop applications and their providers/parsers/utilities.
- `src/vite-env.d.ts`: renderer-facing API and shared global types.
- `src/styles/`: Sass modules, theme tokens, page styles, remote desktop app styles.

## Coding Guidelines

- Use TypeScript strict patterns already present in the codebase.
- Prefer existing React hooks, helpers, naming, and component organization.
- Use renderer APIs through `window.guiSSH`; do not call Node or Electron APIs directly from React components.
- Keep Electron main-process files as CommonJS `.cjs`.
- Keep UI text localizable through `src/i18nCoreCatalog.ts` and `src/i18nCatalog.ts` when it appears on first-load or lazy-loaded screens.
- Keep styles in SCSS modules under `src/styles/`; add both dark and light theme coverage when needed.
- Do not introduce broad refactors in feature or bug-fix PRs unless the refactor is required.

## IPC Changes

When adding or changing IPC APIs, update all relevant layers:

- Main process handler in `electron/main/*Handlers.cjs`
- Bridge API in `electron/preload.cjs`
- Type definitions in `src/vite-env.d.ts`
- Renderer calls and user-facing error text
- Tests or manual verification notes in the pull request

Use the existing `registerIpcHandler` wrapper unless the local pattern intentionally requires a direct `ipcMain.handle`.

## Remote Desktop Apps

When adding a remote desktop application, check all required registration points:

- Component file and barrel export in `src/components/remote-desktop/index.ts`
- `desktopApps`, icon sources, default window frame, and render branch in `src/RemoteDesktopShell.tsx`
- `ShellDeskDesktopAppKey` and related global types in `src/vite-env.d.ts`
- Desktop icon asset under `src/assets/desktop-icons/`
- SCSS module and `src/styles/index.scss`
- Layout allowlist and migration keys in both renderer and main-process constants

Make sure new app keys survive vault sync, config save/load, and app restart.

## Local Test Credentials

If a local `.env` exists, it may contain development-only SSH test variables:

```env
SHELLDESK_TEST_SSH_HOST=
SHELLDESK_TEST_SSH_PORT=
SHELLDESK_TEST_SSH_USERNAME=
SHELLDESK_TEST_SSH_PASSWORD=
```

Never print or commit `SHELLDESK_TEST_SSH_PASSWORD`. If credentials are missing, skip live SSH verification and mention that in the pull request.

## Pull Request Checklist

Before opening a PR:

- Run `pnpm typecheck`.
- Run `pnpm build` for code changes.
- Verify relevant UI flows manually when changing host management, remote desktop apps, IPC, sync, packaging, or theme/layout behavior.
- Update docs, examples, and screenshots when behavior changes.
- Confirm no generated build artifacts, local logs, `.env`, credentials, or private host data are included.

## Commit and PR Style

- Keep PRs focused on one fix or feature.
- Explain user-visible behavior, risk, and verification steps.
- Link related issues.
- Include screenshots or recordings for UI changes.
- Call out platform-specific limitations, especially for Windows/macOS/Linux packaging or remote OS behavior.

