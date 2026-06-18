## Summary

<!-- What changed and why? Keep this short and user-focused. -->

## Related Issue

<!-- Link the issue this PR closes or relates to. Example: Closes #123 -->

## Type of Change

- [ ] Bug fix
- [ ] New feature
- [ ] UI / UX improvement
- [ ] Security / privacy improvement
- [ ] Performance improvement
- [ ] Refactor / maintenance
- [ ] Documentation
- [ ] Build / packaging / release

## Affected Areas

- [ ] Host management / vault
- [ ] SSH connection / jump host
- [ ] Terminal
- [ ] SFTP / file explorer
- [ ] Remote desktop / window manager
- [ ] Browser / webview
- [ ] VNC
- [ ] Database tools
- [ ] Process / service / container tools
- [ ] Network / firewall / disk / package tools
- [ ] AI settings / chat
- [ ] Sync / import / export
- [ ] Build / packaging / release
- [ ] UI / theme / layout
- [ ] Other:

## Screenshots or Recordings

<!-- Add before/after screenshots or recordings for UI changes. Mask sensitive host details. -->

## Testing

- [ ] `pnpm typecheck`
- [ ] `pnpm build`
- [ ] Manual UI verification
- [ ] Live SSH verification
- [ ] Packaging verification
- [ ] Not run; reason:

### Manual Test Notes

<!-- Describe the flows, local OS, remote OS, auth method, jump host setup, or remote app tested. Do not include credentials. -->

## Security and Privacy Checklist

- [ ] No credentials, private keys, passphrases, tokens, API keys, or real production host details are included.
- [ ] `.env` and local vault/config/log files are not included.
- [ ] Logs, screenshots, and recordings are masked.
- [ ] IPC inputs, file paths, remote commands, URLs, and imported config data are validated where relevant.
- [ ] Changes that touch `sudo`, `su root`, SSH, SFTP, VNC, webview, sync, or key storage have been reviewed for security impact.

## Implementation Checklist

- [ ] Renderer changes use `window.guiSSH` instead of direct Node/Tauri APIs.
- [ ] IPC changes update the Rust dispatcher/handler, `src/tauriBridge.ts`, and `src/vite-env.d.ts`.
- [ ] User-facing text is added to the i18n catalogs.
- [ ] Dark and light theme styles are covered.
- [ ] Remote desktop app changes update app registration, icon, window frame, layout persistence, and migration keys when applicable.
- [ ] Documentation or README updates are included when behavior changes.

## Additional Notes

<!-- Call out known limitations, follow-up work, migration behavior, or review areas. -->
