// Copyright © 2026 Jalapeno Labs

import type { AuthFlowState } from './authFlowState'

// UI
import { Button } from '@heroui/react'
import { AuthShell } from './AuthShell'
import { AuthCard } from './AuthCard'

type Props = {
  state: AuthFlowState
  title: string
  body: string
  actionLabel: string
  onAction: () => void
  eyebrow?: string
  showPresence?: boolean
}

// The shared shape for single-message auth screens (signed out, invalid invite,
// access denied, suspended, identity unavailable, session expired): a title, a
// short body, and one action. Screens supply the copy and the action.
export function AuthNotice(props: Props) {
  return <AuthShell state={props.state} showPresence={props.showPresence}>
    <AuthCard>
      { props.eyebrow
        && <p className='mb-2 text-xs uppercase tracking-widest opacity-60'>{ props.eyebrow }</p>
      }
      <h1 className='text-2xl'>
        <strong>{ props.title }</strong>
      </h1>
      <p className='mt-3 opacity-80'>{ props.body }</p>
      <Button
        className='mt-6 w-full'
        color='primary'
        onPress={props.onAction}
      >
        <span>{ props.actionLabel }</span>
      </Button>
    </AuthCard>
  </AuthShell>
}
