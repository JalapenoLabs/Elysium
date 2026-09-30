// Copyright © 2026 Jalapeno Labs

import type { Session } from '@ory/client-fetch'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { Button, Chip, Spinner, toast } from '@heroui/react'
import { LuMonitorSmartphone } from 'react-icons/lu'
import { SecuritySection } from './SecuritySection'

// Misc
import { kratos } from '../../../api/kratos'

const dateFormatter = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' })

// Where this person is signed in, straight from Kratos. Signing a device out ends its
// session; the workspace notices within half a minute.
export function SessionsSection() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const [ isEnding, setIsEnding ] = useState(false)
  const { data: current } = useSWR('kratos/session', () => kratos.toSession())
  const { data: others, error, mutate } = useSWR('kratos/sessions', () => kratos.listMySessions())

  async function endOthers() {
    setIsEnding(true)
    try {
      await kratos.disableMyOtherSessions()
      toast.success(t('security.sessions.othersEnded'))
      await mutate()
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
      await mutate()
    }
    catch (failure) {
      console.debug('SessionsSection could not end a session', { failure, sessionId: session.id })
      toast.danger(t('common:errors.unexpected'))
    }
  }

  // Kratos lists every session but the one asking, so this device comes first, from whoami.
  const sessions: Session[] = []
  if (current) {
    sessions.push(current)
  }
  sessions.push(...(others ?? []))

  return <SecuritySection
    title={t('security.sessions.title')}
    description={t('security.sessions.description')}
    status={<Button
      size='sm'
      variant='outline'
      isDisabled={!others?.length}
      isPending={isEnding}
      onPress={endOthers}
    >
      <span>{t('security.sessions.endOthers')}</span>
    </Button>}
  >
    {error && <p className='text-sm text-danger'>{t('security.sessions.loadError')}</p>}
    {!error && !others && <Spinner size='sm' />}
    <ul className='flex flex-col gap-2'>{
      sessions.map((session) => {
        const device = session.devices?.[0]
        const isCurrent = session.id === current?.id
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
                    ? dateFormatter.format(session.authenticated_at)
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
  </SecuritySection>
}
