// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'

// Redux
import { selectAllConnectors } from '../../../store/connectorsSlice'
import { useAppSelector } from '../../../store/hooks'
import { selectListedMeetings } from '../../../store/meetingsSlice'

// User interface
import { Spinner, Tabs, buttonVariants } from '@heroui/react'
import { PageTabs } from '../../../components/PageTabs'
import { useConnectorKinds } from '../Connectors/useConnectorKinds'
import { SettingsPage } from '../SettingsPage'
import { MeetingList } from './MeetingList'
import { MeetingSources } from './MeetingSources'

// Misc
import { useConnectorsLoader, useMeetingsLoader } from '../../../hooks/useServerData'
import { useTabParam } from '../../../hooks/useTabParam'
import { getConnectorPickerUrl } from '../../../urls'
import { meetingSourcesOf } from './meetingsPresentation'

const TABS = [ 'meetings', 'sources' ] as const

// `/settings/meetings`: the person's recent meetings, with how far each one's processing got,
// and on a tab of its own where they come from. A source is any connector whose kind provides
// meetings, so the page names none of them itself. The open tab lives in the address. See
// docs/meetings.md.
export function MeetingsPage() {
  const { t } = useTranslation([ 'meetings', 'common' ])
  const definitions = useConnectorKinds()
  const connectorsStatus = useConnectorsLoader()
  const connectors = useAppSelector(selectAllConnectors)
  const meetingsStatus = useMeetingsLoader()
  const meetings = useAppSelector(selectListedMeetings)
  const [ selectedTab, selectTab ] = useTabParam(TABS)

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
    <PageTabs
      label={t('tabs.label')}
      selectedKey={selectedTab}
      onSelectionChange={selectTab}
      tabs={[
        {
          id: 'meetings',
          label: t('tabs.meetings'),
          count: meetingsStatus === 'loaded'
            ? meetings.length
            : null,
        },
        {
          id: 'sources',
          label: t('tabs.sources'),
          count: isLoading || hasFailed
            ? null
            : sources.connectors.length,
        },
      ]}
    >
      <Tabs.Panel id='meetings'>
        <MeetingList
          status={meetingsStatus}
          meetings={meetings}
          hasSources={isLoading || hasSources}
        />
      </Tabs.Panel>
      <Tabs.Panel id='sources'>
        {isLoading && <div className='grid place-items-center py-10'>
          <Spinner />
        </div>}
        {!isLoading && hasFailed && <p className='text-sm text-danger'>{t('common:errors.unexpected')}</p>}
        {!isLoading && !hasFailed && <MeetingSources kinds={sources.kinds} connectors={sources.connectors} />}
      </Tabs.Panel>
    </PageTabs>
  </SettingsPage>
}
