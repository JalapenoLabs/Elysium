// Copyright © 2026 Jalapeno Labs

// Core
import { useParams } from 'react-router'
import { useTranslation } from 'react-i18next'

// UI
import { Button } from '@heroui/react'
import { AuthShell } from '../../components/auth/AuthShell'
import { AuthCard } from '../../components/auth/AuthCard'
import { InvalidInvite } from './InvalidInvite'

// Landing for a signed invite link. The real flow verifies the token via
// GET /api/v1/invites/:token, previews the org/team, then accepts by enrolling a
// passkey for the invited account (the same native ceremony as first-run, against
// our API). That endpoint is a later roadmap item (docs/auth-model.md); the token
// is read so the accept action can carry it once it lands.
export function InviteAcceptance() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const params = useParams()

  const inviteToken = params.token ?? ''

  // A signed link always carries a token; an empty one is malformed.
  if (!inviteToken) {
    return <InvalidInvite />
  }

  function onAccept() {
    // Invite verification + passkey enrollment for the invited account is pending
    // its backend endpoint; wired here once that lands.
    console.debug('InviteAcceptance accept (pending backend invite endpoint)', { inviteToken })
  }

  return <AuthShell state='idle'>
    <AuthCard>
      <p className='mb-2 text-xs uppercase tracking-widest opacity-60'>
        { t('auth:invite.accept.eyebrow') }
      </p>
      <h1 className='text-2xl'>
        <strong>{ t('auth:invite.accept.title', { orgName: t('common:appName') }) }</strong>
      </h1>
      <p className='mt-3 opacity-80'>
        { t('auth:invite.accept.body', { inviterName: 'An admin', roleName: 'a member' }) }
      </p>
      <Button
        className='mt-6 w-full'
        color='primary'
        onPress={onAccept}
      >
        <span>{ t('auth:invite.accept.action') }</span>
      </Button>
    </AuthCard>
  </AuthShell>
}
