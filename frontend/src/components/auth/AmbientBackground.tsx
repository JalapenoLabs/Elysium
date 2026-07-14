// Copyright © 2026 Jalapeno Labs

import type { CSSProperties } from 'react'
import type { AuthFlowState } from './authFlowState'

// Core
import { useEffect, useRef } from 'react'

// UI
import { useReducedMotion } from 'framer-motion'

// Misc
import { accentByState, auraSecondaryByState } from './authFlowState'

type Props = {
  state: AuthFlowState
}

// PLACEHOLDER for the signature ambient background asset (see
// docs/auth-art-briefs.md, #1). A pure-CSS stand-in: slow-drifting blurred
// blobs whose hue follows the state-color grammar, leaning gently toward the
// pointer, under a film-grain wash and a contrast scrim that keeps foreground
// text legible. When the commissioned shader/loop lands, it replaces the
// visuals here while keeping this same `state` prop contract.
export function AmbientBackground(props: Props) {
  const fieldRef = useRef<HTMLDivElement>(null)
  const prefersReducedMotion = useReducedMotion()

  // Pointer parallax: written straight to a CSS variable (not React state) so
  // mousemove never causes a re-render; the CSS transition supplies the float.
  useEffect(() => {
    if (prefersReducedMotion) {
      return undefined
    }

    function onPointerMove(event: PointerEvent) {
      const field = fieldRef.current
      if (!field) {
        return
      }
      const ratioX = event.clientX / window.innerWidth - 0.5
      const ratioY = event.clientY / window.innerHeight - 0.5
      field.style.setProperty('--aura-parallax-x', `${(ratioX * 2.5).toFixed(3)}rem`)
      field.style.setProperty('--aura-parallax-y', `${(ratioY * 1.75).toFixed(3)}rem`)
    }

    window.addEventListener('pointermove', onPointerMove)
    return () => window.removeEventListener('pointermove', onPointerMove)
  }, [ prefersReducedMotion ])

  const auraStyle = {
    '--aura-accent': accentByState[props.state],
    '--aura-secondary': auraSecondaryByState[props.state],
  } as CSSProperties

  return <div ref={fieldRef} className='auth-aura' style={auraStyle} aria-hidden='true'>
    <div className='auth-aura__parallax'>
      <div className='auth-aura__blob auth-aura__blob--one' />
      <div className='auth-aura__blob auth-aura__blob--two' />
      <div className='auth-aura__blob auth-aura__blob--three' />
    </div>
    <div className='auth-aura__grain' />
    <div className='auth-aura__scrim' />
  </div>
}
