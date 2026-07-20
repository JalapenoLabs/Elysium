// Copyright © 2026 Jalapeno Labs

// Misc
import { statusTokens } from '../../theme/statusColors'

// The auth flow's stage names, mapped onto Elysium's product-wide status-color
// grammar (src/theme/statusColors.ts, docs/ux.md). Users learn the grammar on
// their very first screen:
//   idle           - calm, waiting
//   authenticating - working (active / orange)
//   needsInput     - your turn, e.g. tap your passkey (pink)
//   error          - something failed (red)
//   success        - done (green)
export type AuthFlowState = 'idle' | 'authenticating' | 'needsInput' | 'error' | 'success'

// The dominant accent per state. The auth surface is always a dark ambient
// stage regardless of the app theme, so it reads the dark-theme hexes directly.
export const accentByState = {
  idle: statusTokens.idle.darkHex,
  authenticating: statusTokens.active.darkHex,
  needsInput: statusTokens.needsInput.darkHex,
  error: statusTokens.error.darkHex,
  success: statusTokens.done.darkHex,
} as const satisfies Record<AuthFlowState, string>

// A secondary hue that pairs with the accent in the ambient field. These are
// cinematic pairings specific to the aura art, not grammar tokens.
export const auraSecondaryByState = {
  idle: '#06b6d4',
  authenticating: '#f59e0b',
  needsInput: '#f472b6',
  error: '#991b1b',
  success: '#4ade80',
} as const satisfies Record<AuthFlowState, string>
