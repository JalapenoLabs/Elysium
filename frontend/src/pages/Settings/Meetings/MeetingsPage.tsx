// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'

// Redux
import { selectAllConnectors } from '../../../store/connectorsSlice'
import { useAppSelector } from '../../../store/hooks'

// User interface
import { Spinner, buttonVariants } from '@heroui/react'
import { useConnectorKinds } from '../Connectors/useConnectorKinds'
import { SettingsPage } from '../SettingsPage'
import { MeetingList } from './MeetingList'
import { MeetingSources } from './MeetingSources'

// Misc
import { useConnectorsLoader } from '../../../hooks/useServerData'
import { getConnectorPickerUrl } from '../../../urls'
import { meetingSourcesOf } from './meetingsPresentation'

// `/settings/meetings`: the person's meeting sources and their recent meetings, with how far
// each one's processing got. A source is any connector whose kind provides meetings, so the
// page names none of them itself. See docs/meetings.md.
export function MeetingsPage() {
  const { t } = useTranslation([ 'meetings', 'common' ])
  const definitions = useConnectorKinds()
  const connectorsStatus = useConnectorsLoader()
  const connectors = useAppSelector(selectAllConnectors)

  const sources = meetingSourcesOf(definitions.kinds, connectors)
  const hasSources = Boolean(sources.connectors.length)
  const isLoading = definitions.isLoading || connectorsStatus === 'loading'
  const hasFailed = Boolean(definitions.error) || connectorsStatus === 'failed'

  return <SettingsPage
    title={t('title')}
    primaryAction={hasSources && <Link
      to={getConnectorPickerUrl('meetings')}
      className={buttonVariants({ variant: 'outline' })}
    >
      <span>{t('sources.add')}</span>
    </Link>}
  >
    {isLoading && <div className='relaxed grid place-items-center py-10'>
      <Spinner />
    </div>}
    {!isLoading && hasFailed && <p className='relaxed text-sm text-danger'>{t('common:errors.unexpected')}</p>}
    {!isLoading && !hasFailed && <MeetingSources kinds={sources.kinds} connectors={sources.connectors} />}
    <MeetingList hasSources={hasSources} />
  </SettingsPage>
}
