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
}

// The shared layout for every auth screen: the living ambient field behind a
// centered, gently-entering content column with the Elysia presence above it.
// Both surfaces (the Keycloakify theme and the app) render through this so the
// seam between them is invisible.
export function AuthShell(props: Props) {
  const showPresence = props.showPresence ?? true

  return <div className='auth-shell'>
    <AmbientBackground state={props.state} />
    <motion.div
      className='auth-shell__content'
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
