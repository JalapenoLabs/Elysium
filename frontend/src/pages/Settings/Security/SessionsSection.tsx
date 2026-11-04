// Copyright © 2026 Jalapeno Labs

import type { Session } from '@ory/client-fetch'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Chip, Spinner } from '@heroui/react'
import { LuLogOut, LuMonitorSmartphone } from 'react-icons/lu'

// Misc
import { formatDateTime } from '../../../format/dates'
import { kratos } from '../../../api/kratos'
import { useSignedInSessions } from './useSignedInSessions'
import { toast } from '../../../toast/toast'

// Where this person is signed in, on the Sessions tab. Signing a device out ends its session;
// the workspace notices within half a minute.
export function SessionsSection() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const [ isEnding, setIsEnding ] = useState(false)
  const signedIn = useSignedInSessions()

  async function endOthers() {
    setIsEnding(true)
    try {
      await kratos.disableMyOtherSessions()
      toast.success(t('security.sessions.othersEnded'))
      await signedIn.reload()
    }
    catch (failure) {
      console.debug('SessionsSection could not end the other sessions', { failure })
      toast.danger(t('common:errors.unexpected'))
    }
    setIsEnding(false)
  }

  async function end(session: Session) {
    try {
      await kratos.disableMySession({ id: session.id })
      toast.success(t('security.sessions.ended'))
      await signedIn.reload()
    }
    catch (failure) {
      console.debug('SessionsSection could not end a session', { failure, sessionId: session.id })
      toast.danger(t('common:errors.unexpected'))
    }
  }

  return <section>
    <div className='compact flex justify-end'>
      <Button
        variant='outline'
        isDisabled={!signedIn.others?.length}
        isPending={isEnding}
        onPress={endOthers}
      >
        <LuLogOut className='size-4' aria-hidden />
        <span>{t('security.sessions.endOthers')}</span>
      </Button>
    </div>
    {signedIn.loadFailed && <p className='text-sm text-danger'>{t('security.sessions.loadError')}</p>}
    {!signedIn.loadFailed && !signedIn.isLoaded && <Spinner size='sm' />}
    <ul className='flex flex-col gap-2'>{
      signedIn.sessions.map((session) => {
        const device = session.devices?.[0]
        const isCurrent = session.id === signedIn.current?.id
        return <li key={session.id} className='level rounded-lg bg-surface-secondary px-4 py-3'>
          <div className='level-left min-w-0'>
            <LuMonitorSmartphone className='size-5 shrink-0 text-accent' aria-hidden />
            <div className='min-w-0'>
              <p className='truncate text-sm font-medium'>{
                device?.user_agent ?? t('security.sessions.unknownDevice')
              }</p>
              <p className='text-xs opacity-60'>{
                t('security.sessions.signedIn', {
                  date: session.authenticated_at
                    ? formatDateTime(session.authenticated_at)
                    : '',
                  address: device?.ip_address ?? '',
                })
              }</p>
            </div>
          </div>
          {isCurrent
            ? <Chip size='sm' variant='soft' color='accent'>{t('security.sessions.thisDevice')}</Chip>
            : <Button size='sm' variant='ghost' onPress={() => end(session)}>
              <span>{t('security.sessions.end')}</span>
            </Button>}
        </li>
      })
    }</ul>
  </section>
}
