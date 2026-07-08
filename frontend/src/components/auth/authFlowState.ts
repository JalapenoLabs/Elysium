// Copyright © 2026 Jalapeno Labs

// Elysium's state-color grammar, used across the whole product and introduced
// here in the auth flow so users learn it on their very first screen.
//   idle           - calm, waiting
//   authenticating - working (orange)
//   needsInput     - your turn, e.g. tap your passkey (pink)
//   error          - something failed (red)
//   success        - done (green)
export type AuthFlowState = 'idle' | 'authenticating' | 'needsInput' | 'error' | 'success'

// TECH DEBT (see docs/Techdebt.md): these hexes are hardcoded and should be driven
// from the Tailwind/HeroUI theme so a theme change updates them in one place, and so
// per-company white-labeling can match customer branding.

// The dominant accent per state.
export const accentByState = {
  idle: '#6366f1',
  authenticating: '#f97316',
  needsInput: '#ec4899',
  error: '#ef4444',
  success: '#22c55e',
} as const satisfies Record<AuthFlowState, string>

// A secondary hue that pairs with the accent in the ambient field.
export const auraSecondaryByState = {
  idle: '#06b6d4',
  authenticating: '#f59e0b',
  needsInput: '#f472b6',
  error: '#991b1b',
  success: '#4ade80',
} as const satisfies Record<AuthFlowState, string>
