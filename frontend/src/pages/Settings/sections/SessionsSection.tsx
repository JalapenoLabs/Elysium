// Copyright © 2026 Jalapeno Labs

import type { Location, Session } from '../../../proto/generated/security_pb'

// Core
import { useState } from 'react'
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// UI
import { Button } from '@heroui/react'
import { SettingsSection } from '../../../components/settings/SettingsSection'
import { OhLookAtTheTime } from '../../../components/OhLookAtTheTime'

// Misc
import { useConfirm } from '../../../hooks/useConfirm'
import { useDisplayTimezone } from '../../../hooks/useDisplayTimezone'
import { revokeOtherSessions, revokeSession } from '../../../api/routes/userRoutes'
import { UrlTree } from '../../../urls'

type Props = {
  sessions: Session[]
  // Reloads the shared security activity after a session is revoked.
  onReload: () => void | Promise<void>
}

// A friendly "Browser on OS" label from a user agent; unknown agents fall back.
function deviceLabel(userAgent: string, unknownLabel: string): string {
  if (!userAgent) {
    return unknownLabel
  }
  const browser = /Edg\//.test(userAgent) ? 'Edge'
    : /OPR\/|Opera/.test(userAgent) ? 'Opera'
      : /Firefox\//.test(userAgent) ? 'Firefox'
        : /Chrome\//.test(userAgent) ? 'Chrome'
          : /Safari\//.test(userAgent) ? 'Safari' : null
  const os = /Windows/.test(userAgent) ? 'Windows'
    : /Mac OS X|Macintosh/.test(userAgent) ? 'macOS'
      : /Android/.test(userAgent) ? 'Android'
        : /iPhone|iPad|iOS/.test(userAgent) ? 'iOS'
          : /Linux/.test(userAgent) ? 'Linux' : null
  if (browser && os) {
    return `${browser} on ${os}`
  }
  return browser ?? os ?? unknownLabel
}

// "City, Country" from best-effort geolocation; empty when unresolved or private.
function locationText(location: Location | undefined): string {
  if (!location) {
    return ''
  }
  return [ location.city, location.country ].filter(Boolean).join(', ')
}

// The Sessions section: an aligned table of live sessions (location, device, IP,
// last-active). The per-row Sign out reveals on hover/focus and confirms through
// the app-wide confirm modal before revoking, so a stray click can't drop a
// session.
export function SessionsSection(props: Props) {
  const { t } = useTranslation('auth')
  const navigate = useNavigate()
  const confirm = useConfirm()
  const displayTimezone = useDisplayTimezone()

  const [ busyId, setBusyId ] = useState<string | null>(null)
  const [ isRevokingOthers, setIsRevokingOthers ] = useState(false)

  async function onRevoke(session: Session) {
    setBusyId(session.id)
    try {
      await revokeSession(session.id)
      // Revoking the current session clears the cookie: this browser is signed
      // out, so send it to the signed-out screen.
      if (session.current) {
        navigate(UrlTree.signedOut)
        return
      }
      await props.onReload()
    }
    finally {
      setBusyId(null)
    }
  }

  async function onRevokeOthers() {
    setIsRevokingOthers(true)
    try {
      await revokeOtherSessions()
      await props.onReload()
    }
    catch (error) {
      console.debug('revoke-others failed', { error })
    }
    finally {
      setIsRevokingOthers(false)
    }
  }

  function confirmRevoke(session: Session) {
    confirm({
      title: t('settings.sessions.confirmTitle'),
      message: session.current
        ? t('settings.sessions.confirmCurrent')
        : t('settings.sessions.confirmOther', {
          device: deviceLabel(session.userAgent, t('settings.sessions.unknownDevice')),
        }),
      confirmLabel: t('settings.sessions.signOut'),
      cancelLabel: t('settings.sessions.keepSession'),
      confirmColor: 'danger',
      onConfirm: () => onRevoke(session),
    })
  }

  const hasOthers = props.sessions.some((session) => !session.current)

  return <SettingsSection
    title={t('settings.sessions.sectionTitle')}
    description={t('settings.sessions.subtitle')}
    action={hasOthers
      ? <Button
        size='sm'
        radius='sm'
        variant='bordered'
        isLoading={isRevokingOthers}
        onPress={onRevokeOthers}
      >
        <span>{ t('settings.sessions.signOutAll') }</span>
      </Button>
      : undefined}
  >
    <table className='w-full text-sm'>
      <thead>
        <tr className='border-b border-divider text-left text-xs uppercase tracking-wide text-default-500'>
          <th className='pb-2 font-medium'>{ t('settings.sessions.colDevice') }</th>
          <th className='pb-2 font-medium'>{ t('settings.sessions.colLocation') }</th>
          <th className='pb-2 font-medium'>{ t('settings.sessions.colIp') }</th>
          <th className='pb-2 font-medium'>{ t('settings.sessions.colActive') }</th>
          <th className='pb-2' />
        </tr>
      </thead>
      <tbody className='divide-y divide-divider'>
        { props.sessions.map((session) =>
          <tr key={session.id} className='group'>
            <td className='py-3'>
              <span>{ deviceLabel(session.userAgent, t('settings.sessions.unknownDevice')) }</span>
              { session.current
                ? <span className='ml-2 text-xs text-success'>{ t('settings.sessions.thisDevice') }</span>
                : null }
            </td>
            <td className='py-3'>
              { locationText(session.location) || <span className='text-default-400'>—</span> }
            </td>
            <td className='py-3 text-default-500'>{ session.ip || '—' }</td>
            <td className='py-3 text-default-500'>
              { session.lastSeenAt
                ? <OhLookAtTheTime timestamp={session.lastSeenAt} relative timezone={displayTimezone} />
                : '—' }
            </td>
            <td className='py-3 text-right'>
              <Button
                size='sm'
                radius='sm'
                variant='light'
                color='danger'
                // Quiet until the row is hovered or the button is focused, so the
                // table reads calm but the action is a keystroke away.
                className='opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100'
                isLoading={busyId === session.id}
                onPress={() => confirmRevoke(session)}
              >
                <span>{ t('settings.sessions.signOut') }</span>
              </Button>
            </td>
          </tr>,
        ) }
      </tbody>
    </table>
  </SettingsSection>
}
