# Navigation and chrome

Elysium's app chrome is a single fixed **bottom taskbar**, modeled on Windows 11.
The workspace deliberately has **no topbar** and never will; vertical space above the
content belongs entirely to the panels.

The framework's own screens keep the stamped `AppShell` and its navbar
([workspace.md](workspace.md) states the seam). The taskbar is the workspace's chrome, not a
replacement for the framework's.

## The taskbar

Three zones on a grid (`1fr auto 1fr`) so the center stays visually centered
regardless of how wide the side zones grow:

| Zone | Contents |
|---|---|
| Left | The **organization and project switcher** (drop-up: the reachable organizations and their projects, organization settings, create), and the **profile badge** (drop-up: other signed-in accounts with one-click switching, add account, my profile, invite people, sign out). Both read the framework's tenancy; neither reimplements it. The multi-account half is blocked on [Anubis #94](https://github.com/JalapenoLabs/Anubis/issues/94). |
| Center | **Search** (an icon that expands into the universal search bar, with a ctrl or cmd+P command palette for jump-to-anything) and the **apps**: Home (the workspace), one launcher per registered workspace panel ([workspace.md](workspace.md)) with a Windows-style running indicator while its dock tab is open, and Settings. Clicking a launcher lands on the workspace and opens or refocuses that panel. |
| Right | The **tray**: manual-status control, theme toggle, clock. |

All drop-ups and popovers open upward. Components live under
`frontend/src/components/taskbar/`.

## Settings are the framework's

Personal, team, and organization settings are Anubis screens, reached from the taskbar
rather than rebuilt in it:

- **Profile and security**: `/settings/profile` and `/settings/security`, from the profile
  badge.
- **Team**: `/teams/:teamId/settings`, plus the team's developers console and audit log.
- **Organization**: `/organizations/:organizationId/settings` and its billing screen, from
  the organization switcher.

Elysium adds project settings alongside them once the project tier lands
([tenancy.md](tenancy.md)).

## Roadmap

- Taskbar app previews and per-launcher close actions, Windows style.
- The search icon and the palette converge into the universal navigator across people,
  messages, files, and repositories, per the action center in CLAUDE.md.
- The tray grows: the notification and fault center, sound mute, and agent activity glances.
  Notifications ride the framework's inbox rather than a second one.
