**Findings**
- No actionable P0/P1/P2 issues remain.

**Open Questions**
- The source visual shows live connection telemetry, OS details, key/proxy names, and relative connection times. The local QA state used form-created preview hosts without opening SSH connections, so those fields render as ready/never connected/unknown system. This is a state-data difference rather than a layout or component fidelity issue.
- The source visual contains varied OS/application glyphs per host. In the local QA state every host is unknown because no remote system probe ran; the implementation now renders unknown systems as a light line icon, and detected systems still use the existing OS icon assets.

**Implementation Checklist**
- Completed: rebuilt the home host screen into a three-panel workbench matching the supplied desktop mock.
- Completed: added the quick SSH command bar, utility controls, host toolbar, dense host table, pagination strip, right detail panel, and bottom status footer.
- Completed: changed the default visual mode to the light Windows-like theme and aligned light tokens with the mock.
- Completed: replaced the homepage sidebar with the mock's visible navigation set and Lucide icon family.
- Completed: removed table overflow, fit 10 host rows in the 1680x945 QA viewport, exposed all three row action buttons, and adjusted table/detail typography.

**Follow-up Polish**
- P3: Real connected-host QA would be useful once test SSH credentials are available, so live status, OS, key, proxy, and connection-duration fields can be compared against the source state.

source visual truth path: `D:\下载\ChatGPT Image 2026年6月13日 12_44_46 (2).png`

implementation screenshot path: `D:\Code\ShellDesk\artifacts\shelldesk-home-implementation-final.png`

viewport: `1680x945`

state: ShellDesk home page, light theme, no active SSH session, 10 temporary preview hosts created through the visible add-host form in the in-app browser. No real credentials were used.

full-view comparison evidence: `D:\Code\ShellDesk\artifacts\shelldesk-home-compare-full.png`

focused region comparison evidence:
- table and toolbar: `D:\Code\ShellDesk\artifacts\shelldesk-home-compare-table.png`
- sidebar: `D:\Code\ShellDesk\artifacts\shelldesk-home-compare-sidebar.png`
- detail panel: `D:\Code\ShellDesk\artifacts\shelldesk-home-compare-detail.png`

patches made since previous QA pass:
- Added `lucide-react` and switched homepage iconography to Lucide where appropriate.
- Reworked `src/App.tsx` homepage JSX around the command bar, host list table, host detail panel, sidebar labels, and selected-host behavior.
- Updated light theme tokens in `src/styles/_tokens.scss`, runtime theme values in `src/App.tsx`, and light overrides in `src/styles/themes/_light.scss`.
- Added homepage layout and footer refresh styles in `src/styles/layout/_app-shell.scss`.
- Added and refined host workbench styles in `src/styles/pages/_hosts.scss`, including row density, icon treatment, table column sizing, action buttons, and detail typography.

final result: passed

**Home Cleanup QA**

**Findings**
- No actionable layout or theme issues remain for the requested cleanup pass.

**Implementation Checklist**
- Completed: left navigation now shows `主机`, `代码片段`, `密钥对`, `已知主机`, `代理`, `日志`, plus Settings; `主机分组` and the sidebar local connection card were removed.
- Completed: quick command bar uses `18px 16px` padding, has no favorite button/dropdown/tools cluster, and places `本地连接` on the right.
- Completed: add-host button matches the adjacent 38px toolbar height and only contains the plus icon plus label.
- Completed: group selection uses an in-app details menu instead of a native select dropdown.
- Completed: list mode removed the status column and reduced actions to centered edit/delete buttons; the action column and table header are sticky.
- Completed: list port column is wider and tag column is narrower.
- Completed: card mode removed favorite/status text/action strip, keeps only the status dot before the host name, and shows `ip:port` in card metadata.
- Completed: dark theme was checked after the cleanup for the shell, command bar, group selector, card surface, and compact card sizing.

browser verification notes:
- temporary QA host: `qa-layout-host`, deleted after verification.
- list action buttons: `28x28`, icons `14x14`.
- add-host button: `38px` high, one SVG icon.
- card height: `136px`.
- dark mode was restored back to light after verification.

validation commands:
- `pnpm typecheck`
- `pnpm build`
- `git diff --check`

cleanup result: passed

**Follow-up QA - Legacy Entries And Dark Theme**
- Completed: kept the new screenshot-driven home layout while restoring legacy entry points in context.
- Completed: `本地连接` is available from the left status card and quick command dropdown.
- Completed: `代码片段` is available from the quick command dropdown.
- Completed: `已知主机` is available from the host toolbar overflow menu.
- Completed: `主机分组` opens an in-page group strip with host counts and filtering instead of replacing the new workbench.
- Completed: checked dark theme colors for the refreshed sidebar, command bar, group strip, table, menus, detail panel, and focus states.

dark theme screenshot path: `D:\Code\ShellDesk\artifacts\shelldesk-home-dark-final.png`

dark theme menu evidence:
- quick command menu: `D:\Code\ShellDesk\artifacts\shelldesk-home-dark-quick-menu.png`
- host toolbar menu: `D:\Code\ShellDesk\artifacts\shelldesk-home-dark-host-menu.png`
- group strip: `D:\Code\ShellDesk\artifacts\shelldesk-home-dark-groups.png`

validation commands:
- `pnpm typecheck`
- `pnpm build`
- `git diff --check`

follow-up result: passed

**Card View QA**

**Findings**
- No actionable P0/P1/P2 issues remain.

**Open Questions**
- The reference card screen uses connected hosts with live status, OS-specific icons, key/proxy names, and historical timestamps. Local QA used 24 temporary hosts created through the visible add-host form without credentials or SSH connections, so cards show `就绪` / `未连接` and unknown-system glyphs. This is a data-state difference, not a layout blocker.
- The QA insertion order differs from the reference because new hosts are prepended by the existing host list behavior. The card grid, page controls, detail panel, and actions were checked independently of that ordering.

**Implementation Checklist**
- Completed: added persisted `grid/list` host view switching backed by `settings.defaultHostView`.
- Completed: added card-mode host rendering with a 4-column responsive grid at the 1680x945 reference viewport.
- Completed: reused existing host actions for connect, terminal/open, edit, info, and delete.
- Completed: added local favorite toggles for card star controls without changing the host vault schema.
- Completed: added shared 20-per-page pagination for list and card modes.
- Completed: verified dark mode for cards, switch controls, pagination, card menus, and the right detail panel.

**Follow-up Polish**
- P3: A future connected-host QA pass can compare detected OS icons and live connection metadata against the source visual more precisely.

source visual truth path: `D:\下载\ChatGPT Image 2026年6月13日 12_44_26 (2).png`

implementation screenshot path: `D:\Code\ShellDesk\artifacts\shelldesk-home-card-light.png`

viewport: `1680x945`

state: ShellDesk home page, card mode, light theme, 24 temporary preview hosts created through the visible add-host form in the in-app browser. No real credentials were used.

full-view comparison evidence: `D:\Code\ShellDesk\artifacts\shelldesk-home-card-compare.png`

focused region comparison evidence:
- dark card mode: `D:\Code\ShellDesk\artifacts\shelldesk-home-card-dark.png`
- dark card menu: `D:\Code\ShellDesk\artifacts\shelldesk-home-card-dark-menu.png`

patches made since the previous QA pass:
- Added `LayoutGrid`, `LayoutList`, and pagination icons for the host view switch and page controls.
- Added host card rendering in `src/App.tsx`, including status, group chips, endpoint metadata, tags, recent connection time, card actions, and menu actions.
- Added local favorite host IDs persisted in browser localStorage.
- Added shared host pagination and page reset behavior for search, group, sort, and view changes.
- Added card-view, switch, pagination, and dark-mode styles in `src/styles/pages/_hosts.scss`.

final result: passed
