// Copyright © 2026 Jalapeno Labs

import type { ActiveSession } from '../../api/routes/userRoutes'

// Core
import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// UI
import { Button, Card, CardBody, Chip } from '@heroui/react'

// Utility
import moment from 'moment-timezone'

// Misc
import {
  getActiveSessions,
  revokeOtherSessions,
  revokeSession,
} from '../../api/routes/userRoutes'
import { UrlTree } from '../../urls'

// A friendly device label from a user agent: browser on OS. Deliberately a small
// heuristic, not a full UA parser; unknown agents fall back to a generic label.
function deviceLabel(userAgent: string, unknownLabel: string): string {
  if (!userAgent) {
    return unknownLabel
  }

  const browser = /Edg\//.test(userAgent)
    ? 'Edge'
    : /OPR\/|Opera/.test(userAgent)
      ? 'Opera'
      : /Firefox\//.test(userAgent)
        ? 'Firefox'
        : /Chrome\//.test(userAgent)
          ? 'Chrome'
          : /Safari\//.test(userAgent)
            ? 'Safari'
            : null

  const os = /Windows/.test(userAgent)
    ? 'Windows'
    : /Mac OS X|Macintosh/.test(userAgent)
      ? 'macOS'
      : /Android/.test(userAgent)
        ? 'Android'
        : /iPhone|iPad|iOS/.test(userAgent)
          ? 'iOS'
          : /Linux/.test(userAgent)
            ? 'Linux'
            : null

  if (browser && os) {
    return `${browser} on ${os}`
  }
  return browser ?? os ?? unknownLabel
}

// Every live session for the account: device, location, and last-active, with
// per-session revoke and a "sign out everywhere else" action. Revoking the
// current session signs this browser out.
export function ActiveSessions() {
  const { t } = useTranslation('auth')
  const navigate = useNavigate()

  const [ sessions, setSessions ] = useState<ActiveSession[]>([])
  const [ isLoading, setIsLoading ] = useState(true)
  const [ busyId, setBusyId ] = useState<string | null>(null)
  const [ isRevokingOthers, setIsRevokingOthers ] = useState(false)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)

  async function loadSessions() {
    try {
      const response = await getActiveSessions()
      setSessions(response.sessions)
    }
    catch (error) {
      console.debug('sessions unavailable', { error })
      setErrorMessage(t('settings.sessions.loadFailed'))
    }
    finally {
      setIsLoading(false)
    }
  }

  useEffect(() => {
    loadSessions()
  }, [])

  async function onRevoke(session: ActiveSession) {
    setBusyId(session.id)
    setErrorMessage(null)
    try {
      await revokeSession(session.id)
      // Revoking the current session clears the cookie: this browser is now
      // signed out, so send it to the signed-out screen.
      if (session.current) {
        navigate(UrlTree.signedOut)
        return
      }
      await loadSessions()
    }
    catch (error) {
      console.debug('session revoke failed', { error })
      setErrorMessage(t('settings.sessions.revokeFailed'))
    }
    finally {
      setBusyId(null)
    }
  }

  async function onRevokeOthers() {
    setIsRevokingOthers(true)
    setErrorMessage(null)
    try {
      await revokeOtherSessions()
      await loadSessions()
    }
    catch (error) {
      console.debug('revoke-others failed', { error })
      setErrorMessage(t('settings.sessions.revokeFailed'))
    }
    finally {
      setIsRevokingOthers(false)
    }
  }

  const hasOthers = sessions.some((session) => !session.current)

  return <section className='w-full'>
    <div className='relaxed'>
      <div className='level items-center'>
        <div>
          <h2 className='text-2xl'>
            <strong>{ t('settings.sessions.title') }</strong>
          </h2>
          <p className='opacity-80'>{ t('settings.sessions.subtitle') }</p>
        </div>
        { hasOthers
          ? <Button
            variant='flat'
            color='danger'
            isLoading={isRevokingOthers}
            isDisabled={isRevokingOthers}
            onPress={onRevokeOthers}
          >
            <span>{ t('settings.sessions.signOutAll') }</span>
          </Button>
          : null }
      </div>
    </div>

    { errorMessage
      ? <p className='compact text-sm text-danger'>{ errorMessage }</p>
      : null }

    { isLoading
      ? <p className='opacity-60'>{ t('settings.sessions.loading') }</p>
      : sessions.map((session) =>
        <div key={session.id} className='compact'>
          <Card className='p-1'>
            <CardBody className='level items-center'>
              <div>
                <div className='flex items-center gap-2'>
                  <strong>{ deviceLabel(session.user_agent, t('settings.sessions.unknownDevice')) }</strong>
                  { session.current
                    ? <Chip size='sm' variant='flat' color='success'>
                      { t('settings.sessions.thisDevice') }
                    </Chip>
                    : null }
                </div>
                <p className='mt-1 text-xs opacity-60'>
                  { [
                    session.ip,
                    t('settings.sessions.lastSeen', { when: moment(session.last_seen_at).fromNow() }),
                  ].filter(Boolean).join(' · ') }
                </p>
              </div>
              <Button
                size='sm'
                variant='light'
                color='danger'
                isLoading={busyId === session.id}
                isDisabled={busyId === session.id}
                onPress={() => onRevoke(session)}
              >
                <span>{ t('settings.sessions.signOut') }</span>
              </Button>
            </CardBody>
          </Card>
        </div>,
      ) }
  </section>
}
