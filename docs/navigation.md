# Navigation & Chrome

Elysium's app chrome is a single fixed **bottom taskbar**, modeled on Windows 11.
The app deliberately has **no topbar** and never will; vertical space above the
content belongs entirely to the workspace.

## The taskbar

Three zones on a grid (`1fr auto 1fr`) so the center stays visually centered
regardless of how wide the side zones grow:

| Zone | Contents |
|---|---|
| Left | **Org switcher** (drop-up: the active account's orgs, one-click `POST /api/v1/session/switch-org`, organization settings, create-org modal against `POST /api/v1/orgs`; the trigger shows the org logo when set, else a building icon) and the **profile badge** (drop-up: other signed-in accounts with one-click switching, add account, identity block, my profile, invite people, sign out). |
| Center | **Search** (an icon that expands into the universal search bar; the ctrl/cmd+P command palette covers jump-to-anything today) and the **apps**: Home (the workspace), one launcher per registered workspace panel (`docs/workspace.md`) with a Windows-style running indicator while its dock tab is open, and Settings. Clicking a launcher lands on the workspace and opens or refocuses that panel. |
| Right | The **tray**: manual-status control, theme toggle, clock. |

All drop-ups/popovers open upward (`top-*` placements). Components live under
`frontend/src/components/taskbar/`.

## Settings: two separate hubs

Personal and organization settings are deliberately distinct surfaces:

- **Profile settings** (`/settings`, `pages/Settings/`): personal account
  concerns, a simple flat sidebar (Profile, Security, Sessions). Reached from the
  profile badge's "My profile".
- **Organization settings** (`/organization`, `pages/Organization/`): org-level
  concerns, a grouped accordion sidebar (General -> Identity & branding; People
  -> Members, Teams). Reached from the org switcher's "Organization settings".

## Roadmap

- Taskbar app previews and per-launcher close actions, Windows style.
- The search icon and the ctrl+p palette converge into the universal navigator
  (people, messages, files, repos) per the action-center vision in CLAUDE.md.
- Tray grows: notification/error center, sound mute, agent activity glances.
