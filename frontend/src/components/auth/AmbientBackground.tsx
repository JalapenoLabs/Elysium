// Copyright © 2026 Jalapeno Labs

import type { CSSProperties } from 'react'
import type { AuthFlowState } from './authFlowState'

// Misc
import { accentByState, auraSecondaryByState } from './authFlowState'

type Props = {
  state: AuthFlowState
}

// PLACEHOLDER for the signature ambient background asset (see
// docs/auth-art-briefs.md, #1). This is a pure-CSS stand-in: two slow-drifting
// blurred blobs whose hue follows the state-color grammar, over a dark scrim that
// keeps foreground text legible. When the commissioned shader/loop lands, it
// replaces the visuals here while keeping this same `state` prop contract.
export function AmbientBackground(props: Props) {
  const auraStyle = {
    '--aura-accent': accentByState[props.state],
    '--aura-secondary': auraSecondaryByState[props.state],
  } as CSSProperties

  return <div className='auth-aura' style={auraStyle} aria-hidden='true'>
    <div className='auth-aura__blob auth-aura__blob--one' />
    <div className='auth-aura__blob auth-aura__blob--two' />
    <div className='auth-aura__scrim' />
  </div>
}
