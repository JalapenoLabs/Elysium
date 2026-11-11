// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { Link, Navigate, useSearchParams } from 'react-router'

// Redux
import { selectAllConnectors } from '../../../store/connectorsSlice'
import { useAppSelector } from '../../../store/hooks'

// User interface
import { Spinner, buttonVariants } from '@heroui/react'
import { useConnectorKinds } from '../Connectors/useConnectorKinds'
import { SettingsPage } from '../SettingsPage'
import { MeetingSources } from './MeetingSources'

// Misc
import { useConnectorsLoader } from '../../../hooks/useServerData'
import { getConnectorPickerUrl, TAB_PARAM, UrlTree } from '../../../urls'
import { meetingSourcesOf } from './meetingsPresentation'

// The tab this page's address once opened the meetings on, before they moved into the knowledge
// base, which a link or bookmark may still carry.
const MEETINGS_TAB = 'meetings'

// `/settings/meetings`: where the person's meetings come from. A source is any connector whose kind
// provides meetings, so the page names none of them itself. The meetings themselves are daily work,
// listed on the knowledge base's Meetings page. See docs/meetings.md.
export function MeetingsPage() {
  const { t } = useTranslation([ 'meetings', 'common' ])
  const [ searchParams ] = useSearchParams()
  const definitions = useConnectorKinds()
  const connectorsStatus = useConnectorsLoader()
  const connectors = useAppSelector(selectAllConnectors)

  const sources = meetingSourcesOf(definitions.kinds, connectors)
  const isLoading = definitions.isLoading || connectorsStatus === 'loading'
  const hasFailed = Boolean(definitions.error) || connectorsStatus === 'failed'

  if (searchParams.get(TAB_PARAM) === MEETINGS_TAB) {
    return <Navigate to={UrlTree.knowledgeMeetings} replace />
  }

  return <SettingsPage
    title={t('sources.title')}
    primaryAction={Boolean(sources.connectors.length) && <Link
      to={getConnectorPickerUrl('meetings')}
      className={buttonVariants({ variant: 'outline' })}
    >
      <span>{t('sources.add')}</span>
    </Link>}
  >
    {isLoading && <div className='grid place-items-center py-10'>
      <Spinner />
    </div>}
    {!isLoading && hasFailed && <p className='text-sm text-danger'>{t('common:errors.unexpected')}</p>}
    {!isLoading && !hasFailed && <MeetingSources kinds={sources.kinds} connectors={sources.connectors} />}
  </SettingsPage>
}
