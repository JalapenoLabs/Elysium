// Copyright © 2026 Jalapeno Labs

// Core
import { useParams } from 'react-router'
import { useTranslation } from 'react-i18next'

// UI
import { Button } from '@heroui/react'
import { AuthShell } from '../../components/auth/AuthShell'
import { AuthCard } from '../../components/auth/AuthCard'
import { InvalidInvite } from './InvalidInvite'

// Misc
import { useSession } from '../../auth/useSession'

// Landing for a signed invite link. The real flow verifies the token via
// GET /api/v1/invites/:token, previews the org/team, then accepts and hands off
// to Keycloak enrollment. The org/inviter/role shown here are placeholders until
// that endpoint lands; the token is read so the accept action can carry it.
export function InviteAcceptance() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const params = useParams()
  const session = useSession()

  const inviteToken = params.token ?? ''

  // A signed link always carries a token; an empty one is malformed.
  if (!inviteToken) {
    return <InvalidInvite />
  }

  function onAccept() {
    console.debug('InviteAcceptance accept (pending backend verify)', { inviteToken })
    // Accepting provisions the user and enters Keycloak enrollment.
    session.signIn()
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
