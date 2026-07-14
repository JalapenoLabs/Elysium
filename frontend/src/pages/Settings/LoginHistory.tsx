// Copyright © 2026 Jalapeno Labs

import type { LoginEvent } from '../../api/routes/userRoutes'

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

// UI
import { Card, CardBody, Chip } from '@heroui/react'

// Utility
import moment from 'moment-timezone'

// Misc
import { getLoginHistory } from '../../api/routes/userRoutes'

// Successful outcomes read calm; everything else (failed, locked, ...) reads as
// a warning worth noticing in the access log.
const chipColorByOutcome = {
  success: 'success',
} as const

// The account-access log: recent sign-in attempts for this account, so a user
// can spot access they don't recognize. Read-only.
export function LoginHistory() {
  const { t } = useTranslation('auth')

  const [ events, setEvents ] = useState<LoginEvent[]>([])
  const [ isLoading, setIsLoading ] = useState(true)

  useEffect(() => {
    let cancelled = false
    getLoginHistory()
      .then((response) => {
        if (!cancelled) {
          setEvents(response.events)
        }
      })
      .catch((error) => {
        console.debug('login history unavailable', { error })
      })
      .finally(() => {
        if (!cancelled) {
          setIsLoading(false)
        }
      })
    return () => {
      cancelled = true
    }
  }, [])

  return <div>
    <h3 className='text-lg'>
      <strong>{ t('settings.security.historyTitle') }</strong>
    </h3>
    <p className='mt-1 opacity-70'>{ t('settings.security.historyBody') }</p>

    <div className='mt-3'>
      { isLoading
        ? <p className='opacity-60'>{ t('settings.security.historyLoading') }</p>
        : events.length
          ? events.map((event, index) =>
            <div key={index} className='compact'>
              <Card className='p-1'>
                <CardBody className='level items-center'>
                  <div>
                    <div className='flex items-center gap-2'>
                      <strong>{ t(`settings.security.method.${event.method}`, event.method) }</strong>
                      <Chip
                        size='sm'
                        variant='flat'
                        color={chipColorByOutcome[event.outcome as keyof typeof chipColorByOutcome] ?? 'warning'}
                      >
                        { t(`settings.security.outcome.${event.outcome}`, event.outcome) }
                      </Chip>
                    </div>
                    <p className='mt-1 text-xs opacity-60'>
                      { [ event.ip, event.user_agent ].filter(Boolean).join(' · ') }
                    </p>
                  </div>
                  <span className='text-sm opacity-70'>
                    { moment(event.created_at).format('MMM D, h:mm A') }
                  </span>
                </CardBody>
              </Card>
            </div>,
          )
          : <p className='opacity-60'>{ t('settings.security.historyEmpty') }</p> }
    </div>
  </div>
}
