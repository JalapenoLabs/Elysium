// Copyright © 2026 Jalapeno Labs

import type { CSSProperties } from 'react'
import type { AuthFlowState } from './authFlowState'

// Misc
import { accentByState } from './authFlowState'

type Props = {
  state: AuthFlowState
  // `needsInput` leans in more assertively to signal "your turn".
  className?: string
}

// PLACEHOLDER for the Elysia presence geometry (see docs/auth-art-briefs.md, #2).
// A layered, pulsing orb that nods at the workspace icosahedron and reacts to the
// flow via the state-color grammar. The real parametric/Lottie asset replaces the
// rings here while keeping this `state` prop contract.
export function ElysiaPresence(props: Props) {
  const presenceStyle = {
    '--presence-accent': accentByState[props.state],
  } as CSSProperties

  return <div
    className={`auth-presence auth-presence--${props.state} ${props.className ?? ''}`}
    style={presenceStyle}
    aria-hidden='true'
  >
    <div className='auth-presence__ring auth-presence__ring--outer' />
    <div className='auth-presence__ring auth-presence__ring--middle' />
    <div className='auth-presence__core' />
  </div>
}
