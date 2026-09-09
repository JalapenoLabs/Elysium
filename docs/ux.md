# UX & Design Language

The design system every Elysium surface builds from. A new surface should never invent a
token, a color, a state pattern, or a keyboard convention: it picks them up from here. The
language generalizes to every surface, including the framework's own screens, which pick it
up through the theme rather than being rewritten
([Anubis #88](https://github.com/JalapenoLabs/Anubis/issues/88)).

## Cinematic direction

Futuristic, artistic, sci-fi, with cinematic game-title energy: huge type, staged
transitions, a world that reacts. Premium, never loud or gamer-neon; it must read as a
product that has existed for years. Concretely, derived from the Gate:

- **Hero typography over chrome.** The most important word on a screen is allowed to be
  enormous (`clamp()`-scaled, uppercase, wide tracking) while everything around it stays
  quiet. Screens have one focal point.
- **A living field, not a flat page.** Full-bleed surfaces (auth, faults, future workspace
  backdrops) sit on the ambient aura (drifting state-colored blobs, film grain, a
  legibility scrim). In-app surfaces stay calm; the drama belongs to full-bleed moments.
- **Staged flows.** Multi-step interactions are page-turns (AnimatePresence transitions),
  not accumulating form sections. One stage, one job.
- **The world reacts.** State changes recolor the environment (the aura, the Elysia
  presence) rather than only swapping a message string.

## The status-color grammar

One color language for the whole product, learned on the very first screen:

| Kind | Meaning | Hue |
|---|---|---|
| `idle` | calm, waiting, nothing demanded | indigo |
| `active` | working, running, in progress | orange |
| `needsInput` | paused on the user; their turn | pink |
| `error` | failed, needs attention | red |
| `done` | finished successfully | green |

Codified as tokens, defined once, consumed three ways:

- **CSS variables**: `--status-idle|active|needs-input|error|done` in
  `frontend/src/index.css`, HeroUI-style `H S% L%` triples. Light values on `:root`, dark
  overrides on `.dark`.
- **Tailwind colors**: `status-idle`, `status-active`, `status-needs-input`,
  `status-error`, `status-done` (alpha works: `bg-status-active/15`).
- **TypeScript mirror**: `frontend/src/theme/statusColors.ts` (`StatusKind`,
  `statusTokens`) for JS consumers that need concrete values (gradients, canvas). A unit
  test (`statusColors.test.ts`) keeps the CSS and TS sides in sync; neither can drift.

Rules: never hardcode a status hex anywhere else; never use these hues decoratively (an
orange illustration is fine, an orange button that is not "active" is not); presence
colors (online/away/offline) and org white-label accents are separate systems.

## Theming

- Light and dark are both first-class; `darkMode: 'class'` with HeroUI semantic colors
  doing the heavy lifting. Never hardcode black/white/gray text: use the theme foreground
  and reduce emphasis with opacity (`opacity-70`, `opacity-50`).
- **White-labeling**: the active org's accent overrides `--heroui-primary` app-wide via
  `useOrgBranding()` (`frontend/src/theme/orgBranding.ts`). Anything "brand primary" must
  read the variable, never a literal.
- Full-bleed cinematic surfaces (the Gate, faults) are always the dark ambient field
  regardless of app theme; they read the dark-theme status hexes directly.

## Typography scale

- **Hero**: `clamp(3.5rem, 13vw, 10rem)`, weight 800, uppercase, wide tracking. One per
  screen, cinematic surfaces only.
- **Prompt**: `clamp(2.5rem, 8vw, 6rem)`, weight 800. Stage instructions.
- **Kicker / identity lines**: `0.875rem`, uppercase, letter-spacing `0.35em` to `0.5em`,
  reduced opacity. Quiet context above or below a focal point.
- **In-app headings**: HeroUI defaults (`text-2xl`/`text-xl` with `font-bold` or
  `<strong>`); pages lead with one heading plus a one-line subtitle at `opacity-80`.
- **Body**: `text-sm`/`text-base` theme foreground; secondary detail at `opacity-50` to
  `opacity-70`, never gray literals.

## Spacing & layout

- `relaxed` (1.5rem below) between sections and larger components; `compact` (0.75rem
  below) between form fields. The class goes on each item, never the wrapper.
- `level` / `level-right` for horizontal rows.
- Forms and reading content bound their width (`max-w-2xl` in settings-style pages) so
  actions never drift far from inputs on wide monitors.
- Chrome is minimal: the bottom taskbar is the only persistent chrome
  ([navigation.md](navigation.md)); vertical space belongs to the workspace.

## Motion

- **Ambient**: slow drift (20s+), only on cinematic surfaces, never competing with a form.
- **Transitions**: state recolors over ~600ms ease; stage page-turns are quick
  (200-400ms); micro-feedback (glyph pop, flash decay) under 250ms.
- **Meaningful pulse**: `active` and `needsInput` breathe; terminal states hold still.
- **`prefers-reduced-motion` is honored everywhere**: decoration stops, functional signals
  (the caret) may keep moving. Tailwind `motion-safe:` gates animation classes; bespoke
  CSS animations get an explicit reduced-motion block.

## Standard state patterns

Every data surface uses the same four patterns (`frontend/src/components/`):

- **Empty** (`states/EmptyState`): quiet and reassuring, never an error. Dimmed icon,
  short headline, optional guidance line, at most one primary action.
- **Loading** (`states/LoadingState`): centered spinner plus an optional quiet label.
  Reserved for genuinely blocking loads.
- **Streaming** (`streaming/StreamingSources`): the default for fan-out work (federated
  Drive search, multi-service sweeps, [storage.md](storage.md)). Each source reports its own
  `StatusKind`; fast sources light `done` instantly, slow ones stay `active`, a failed
  source turns `error` with a one-line detail while healthy results keep rendering.
  Results appear as they arrive; nothing waits for the slowest source.
- **Error** (`states/ErrorState`): the red grammar dot, calm copy, a retry when the caller
  can retry. Never a stack trace, never a wall of text. Fault pages (denied, suspended,
  invalid invite) are full-screen cinematic variants of the same idea.

## Keyboard-first

"Hands never leave the keyboard" is a product goal.

- **Command palette, ctrl/cmd+P** (`components/palette/`): the universal navigator. One
  modal, fuzzy-ranked (uikit `fuzzyMatch`), arrows + Enter + Escape. Pillars contribute
  entries through `registerPaletteEntries` / `usePaletteEntries` (module registry); the
  palette knows none of them. Core navigation registers from `AppLayout`. The roadmap
  grows this into the universal searchbar (people, messages, files, repos).
- **Hotkeys** use uikit `useHotkey`; chords that shadow browser defaults must
  `preventDefault`.
- **Forms**: real `<form>` submit on Enter, first field focused on mount, submit disabled
  until valid with a tooltip explaining why (see the global React engineering doc conventions).
- Focus is always visible; anything mouse-reachable is keyboard-reachable.

## Shared component inventory

Reuse before building; contribute sharable pieces to `@jalapenolabs/uikit` rather than
duplicating (`CLAUDE.md`). Elysium-specific primitives:

| Component | Purpose |
|---|---|
| `status/StatusIndicator` | The grammar dot + label |
| `states/EmptyState`, `LoadingState`, `ErrorState` | The standard state patterns |
| `streaming/StreamingSources` | Per-source status strip for fan-out work |
| `form/SecretInput` | Masked credential field with reveal toggle |
| `form/KeyValueEditor` | Controlled key/value config rows (secret mode available) |
| `form/SubmitButton` | Standard submit affordance |
| `palette/CommandPalette` + registry | ctrl/cmd+P navigator with pluggable entries |
| `settings/SettingsSection` + `DetailRow` | Stripe-style label/value read+edit sections |
| `enrollment/*` | Method chooser, TOTP enroll, recovery codes |
| `gates/ConfirmGate` + `useConfirm`, `PromptGate` + `usePrompt` | App-wide confirm/prompt modals |
| `OhLookAtTheTime` | Timezone-aware timestamp element |

From uikit: `Button`, `StatusChip`, `Loader`, `Accordion`, `DragNDrop`, `LuminescentText`,
`useHotkey`, `usePagination`, `fuzzyMatch`, `HighlightFuzzy`, `formatBytes`,
`parseJsonSafe`, and the rest of its utils. HeroUI remains the base kit for inputs,
tables, modals, and chips inside the app theme.

## Roadmap

- The rc-dock workspace shell and embedded-panel chrome build on this language; panels adopt
  the status grammar for their chrome status.
- Shared primitives move upstream into uikit rather than being rebuilt here.
- The palette grows result providers (people, files, messages) as those pillars land.
