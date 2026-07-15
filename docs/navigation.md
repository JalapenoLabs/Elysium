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
| Center | **Search** (an icon that expands into the universal search bar; results land with the universal search / ctrl+p work) and the **apps**: the running rc-dock panels once the workspace lands. Until then, the routed areas (Home, About, Settings) stand in, with a Windows-style active indicator bar. |
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

- The center zone swaps its stand-in route buttons for live rc-dock panel
  entries when the dockable workspace ships (each open panel = a taskbar app,
  with previews and close actions like Windows).
- The search expands into the universal ctrl+p navigator (people, messages,
  files, repos) per the action-center vision in CLAUDE.md.
- Tray grows: notification/error center, sound mute, agent activity glances.
