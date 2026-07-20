// Copyright © 2026 Jalapeno Labs

// Elysium's product-wide status-color grammar (docs/ux.md):
//   idle       - calm, waiting, nothing demanded of anyone (indigo)
//   active     - working / running / in progress (orange)
//   needsInput - paused on the user; their turn to act (pink)
//   error      - something failed and needs attention (red)
//   done       - finished successfully (green)
//
// The rendered values live as CSS variables (`--status-*` in index.css, HeroUI-style
// `H S% L%` triples so Tailwind can compose alpha like `bg-status-active/15`), with a
// light and a dark definition each. This file is the TypeScript mirror for consumers
// that need concrete color values (canvas, gradients, non-CSS renderers); a unit test
// keeps the two sides in sync, so neither can drift silently.
export type StatusKind = 'idle' | 'active' | 'needsInput' | 'error' | 'done'

type StatusToken = {
  // Concrete hexes per theme, for JS consumers that cannot read CSS variables.
  lightHex: string
  darkHex: string
  // The theme-following CSS variable declared in index.css.
  cssVar: string
}

export const statusTokens = {
  idle: {
    lightHex: '#4f46e5',
    darkHex: '#6366f1',
    cssVar: '--status-idle',
  },
  active: {
    lightHex: '#ea580c',
    darkHex: '#f97316',
    cssVar: '--status-active',
  },
  needsInput: {
    lightHex: '#db2777',
    darkHex: '#ec4899',
    cssVar: '--status-needs-input',
  },
  error: {
    lightHex: '#dc2626',
    darkHex: '#ef4444',
    cssVar: '--status-error',
  },
  done: {
    lightHex: '#16a34a',
    darkHex: '#22c55e',
    cssVar: '--status-done',
  },
} as const satisfies Record<StatusKind, StatusToken>

export const statusKinds = Object.keys(statusTokens) as StatusKind[]
