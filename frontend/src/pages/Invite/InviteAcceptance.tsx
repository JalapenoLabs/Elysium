// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useState } from 'react'
import { useNavigate, useParams } from 'react-router'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'

// UI
import { Button, Spinner } from '@heroui/react'
import { AuthShell } from '../../components/auth/AuthShell'
import { AuthCard } from '../../components/auth/AuthCard'
import { InvalidInvite } from './InvalidInvite'
import { InviteEnroll } from './InviteEnroll'

// Misc
import { inviteAcceptJoin, previewInvite } from '../../api/routes/authRoutes'
import { resolveSession } from '../../auth/session'
import { UrlTree } from '../../urls'

// What the public preview endpoint reveals about a valid invite.
export type InvitePreview = {
  org_name: string
  team_name: string
  role: string
  email: string
}

// Landing for a signed invite link. The token is verified via the public
// preview (a missing/expired/used token is a generic 404 -> the invalid view).
// An anonymous visitor enrolls a credential to accept (the same method-aware
// ceremony as first-run claim); a signed-in visitor joins the org with one
// click instead (their existing profile simply gains the membership).
export function InviteAcceptance() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const navigate = useNavigate()
  const params = useParams()
  const sessionStatus = useAppSelector((state) => state.session.status)

  const inviteToken = params.token ?? ''

  const [ preview, setPreview ] = useState<InvitePreview | null>(null)
  const [ isInvalid, setIsInvalid ] = useState(false)
  const [ isJoining, setIsJoining ] = useState(false)
  const [ joinError, setJoinError ] = useState<string | null>(null)

  useEffect(() => {
    if (!inviteToken) {
      return undefined
    }
    let cancelled = false
    previewInvite(inviteToken)
      .then((invite) => {
        if (!cancelled) {
          setPreview(invite)
        }
      })
      .catch((error) => {
        // Missing, expired, and already-used tokens are indistinguishable by
        // design; they all land on the invalid view.
        console.debug('invite preview failed', { error })
        if (!cancelled) {
          setIsInvalid(true)
        }
      })
    return () => {
      cancelled = true
    }
  }, [ inviteToken ])

  // A signed link always carries a token; an empty one is malformed.
  if (!inviteToken || isInvalid) {
    return <InvalidInvite />
  }

  if (!preview || sessionStatus === 'loading' || sessionStatus === 'unknown') {
    return <AuthShell state='idle'>
      <AuthCard>
        <div className='flex items-center justify-center gap-3 py-6'>
          <Spinner size='sm' />
          <p className='opacity-80'>{ t('auth:invite.accept.verifying') }</p>
        </div>
      </AuthCard>
    </AuthShell>
  }

  // Signed in: the profile already exists, so accepting is just joining.
  if (sessionStatus === 'authenticated') {
    async function onJoin() {
      setIsJoining(true)
      setJoinError(null)
      try {
        await inviteAcceptJoin(inviteToken)
        // Reseed so the new org shows up everywhere, then land home.
        await resolveSession()
        navigate(UrlTree.home)
      }
      catch (error) {
        console.debug('invite join failed', { error })
        setJoinError(t('auth:invite.accept.joinFailed'))
      }
      finally {
        setIsJoining(false)
      }
    }

    return <AuthShell state='idle'>
      <AuthCard>
        <p className='mb-2 text-xs uppercase tracking-widest opacity-60'>
          { t('auth:invite.accept.eyebrow') }
        </p>
        <h1 className='text-2xl'>
          <strong>{ t('auth:invite.accept.title', { orgName: preview.org_name }) }</strong>
        </h1>
        <p className='mt-3 opacity-80'>
          { t('auth:invite.accept.joinBody', {
            orgName: preview.org_name,
            roleName: t(`auth:settings.members.roles.${preview.role}`, preview.role),
          }) }
        </p>
        { joinError
          ? <p className='mt-3 text-sm text-danger'>{ joinError }</p>
          : null }
        <Button
          className='mt-6 w-full'
          color='primary'
          isLoading={isJoining}
          onPress={onJoin}
        >
          <span>{ t('auth:invite.accept.joinAction') }</span>
        </Button>
      </AuthCard>
    </AuthShell>
  }

  return <InviteEnroll
    token={inviteToken}
    preview={preview}
  />
}
