// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type { AuthFlowState } from './authFlowState'

// UI
import { motion } from 'framer-motion'
import { AmbientBackground } from './AmbientBackground'
import { ElysiaPresence } from './ElysiaPresence'

type Props = {
  state: AuthFlowState
  children: ReactNode
  // Presence shows on ceremony screens; settings/fault screens can hide it.
  showPresence?: boolean
  // 'column' is the classic centered card column; 'stage' is the full-bleed
  // cinematic layout the login gate uses.
  variant?: 'column' | 'stage'
}

// The shared layout for every auth screen: the living ambient field behind a
// centered, gently-entering content column with the Elysia presence above it.
// Every auth surface renders through this so the whole flow feels like one place.
export function AuthShell(props: Props) {
  const showPresence = props.showPresence ?? true
  const variant = props.variant ?? 'column'

  return <div className='auth-shell'>
    <AmbientBackground state={props.state} />
    <motion.div
      className={`auth-shell__content ${variant === 'stage' ? 'auth-shell__content--stage' : ''}`}
      initial={{ opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4, ease: 'easeOut' }}
    >
      { showPresence
        && <ElysiaPresence state={props.state} className='auth-shell__presence' />
      }
      { props.children }
    </motion.div>
  </div>
}
