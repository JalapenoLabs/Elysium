// Copyright © 2026 Jalapeno Labs

import type { Location, SignInEvent } from '../../../proto/generated/security_pb'

// Core
import { useTranslation } from 'react-i18next'

// UI
import { Chip } from '@heroui/react'
import { SettingsSection } from '../../../components/settings/SettingsSection'
import { OhLookAtTheTime } from '../../../components/OhLookAtTheTime'

// Misc
import { useDisplayTimezone } from '../../../hooks/useDisplayTimezone'

type Props = {
  events: SignInEvent[]
}

// "City, Country" from best-effort geolocation; empty when unresolved or private.
function locationText(location: Location | undefined): string {
  if (!location) {
    return ''
  }
  return [ location.city, location.country ].filter(Boolean).join(', ')
}

// The account-access log: recent sign-in attempts, so a user can spot access
// they don't recognize. A flat aligned table, most recent first.
export function LoginHistorySection(props: Props) {
  const { t } = useTranslation('auth')
  const displayTimezone = useDisplayTimezone()

  return <SettingsSection
    title={t('settings.security.historyTitle')}
    description={t('settings.security.historyBody')}
  >
    { props.events.length
      ? <table className='w-full text-sm'>
        <thead>
          <tr className='border-b border-divider text-left text-xs uppercase tracking-wide text-default-500'>
            <th className='pb-2 font-medium'>{ t('settings.security.colMethod') }</th>
            <th className='pb-2 font-medium'>{ t('settings.security.colLocation') }</th>
            <th className='pb-2 font-medium'>{ t('settings.security.colFrom') }</th>
            <th className='pb-2 font-medium'>{ t('settings.security.colWhen') }</th>
          </tr>
        </thead>
        <tbody className='divide-y divide-divider'>
          { props.events.map((event, index) =>
            <tr key={index}>
              <td className='py-3'>
                <span>{ t(`settings.security.method.${event.method}`, event.method) }</span>
                <Chip
                  size='sm'
                  variant='flat'
                  radius='sm'
                  className='ml-2'
                  color={event.outcome === 'success' ? 'success' : 'warning'}
                >
                  { t(`settings.security.outcome.${event.outcome}`, event.outcome) }
                </Chip>
              </td>
              <td className='py-3'>
                { locationText(event.location) || <span className='text-default-400'>—</span> }
              </td>
              <td className='py-3 text-default-500'>{ event.ip || '—' }</td>
              <td className='py-3 text-default-500'>
                { event.createdAt
                  ? <OhLookAtTheTime timestamp={event.createdAt} timezone={displayTimezone} />
                  : '—' }
              </td>
            </tr>,
          ) }
        </tbody>
      </table>
      : <p className='text-sm text-default-500'>{ t('settings.security.historyEmpty') }</p> }
  </SettingsSection>
}
