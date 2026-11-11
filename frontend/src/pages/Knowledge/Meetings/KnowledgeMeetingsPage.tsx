// Copyright © 2026 Jalapeno Labs

import type { SmartTableFilter } from '@jalapenolabs/uikit'
import type { Meeting } from '../../../api/routes/meetingRoutes'

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useSearchParams } from 'react-router'

// Redux
import { selectAllConnectors } from '../../../store/connectorsSlice'
import { useAppSelector } from '../../../store/hooks'
import { selectListedMeetings } from '../../../store/meetingsSlice'
import { selectAllProjects } from '../../../store/projectsSlice'

// User interface
import { buttonVariants, Chip } from '@heroui/react'
import { LuSettings, LuUpload } from 'react-icons/lu'
import { Page } from '../../../layout/page/Page'
import { MeetingList } from './MeetingList'

// Misc
import { MEETING_STATES } from '../../../api/routes/meetingRoutes'
import { formatNumber } from '../../../format/numbers'
import { useConnectorsLoader, useMeetingsLoader, useProjectsLoader } from '../../../hooks/useServerData'
import { UrlTree } from '../../../urls'
import { NO_PROJECT } from '../../ActionItems/actionItemFilters'
import { useConnectorKinds } from '../../Settings/Connectors/useConnectorKinds'
import {
  meetingSourceNameKey,
  meetingSourcesOf,
  meetingStateLabelKeys,
} from '../../Settings/Meetings/meetingsPresentation'
import {
  filterMeetings,
  fromMeetingFilterValues,
  listMeetingSourceKinds,
  MEETING_FILTER_IDS,
  readMeetingFilters,
  toMeetingFilterValues,
  writeMeetingFilters,
} from './meetingFilters'

// `/knowledge/meetings`: the knowledge base's Meetings page, every meeting the person can see,
// newest first, with its search, filters by source, state, and project, and how many wait for their
// review. It is each meeting's home, those on no project included, and where uploads start
// (`docs/meeting-review.md`, In the knowledge base). Where meetings come from is set up under
// Settings, Meetings, which its one action opens. The filters live in the address, so a project's
// Meetings group links here.
export function KnowledgeMeetingsPage() {
  const { t } = useTranslation([ 'meetings', 'knowledge', 'connectors' ])
  const [ searchParams, setSearchParams ] = useSearchParams()
  const filters = readMeetingFilters(searchParams)
  const status = useMeetingsLoader()
  const meetings = useAppSelector(selectListedMeetings)
  useProjectsLoader()
  const projects = useAppSelector(selectAllProjects)
  const definitions = useConnectorKinds()
  const connectorsStatus = useConnectorsLoader()
  const connectors = useAppSelector(selectAllConnectors)

  // Until both have loaded, an empty list says only that there are no meetings yet.
  const hasSources = definitions.isLoading
    || connectorsStatus === 'loading'
    || Boolean(meetingSourcesOf(definitions.kinds, connectors).connectors.length)
  const { matches, waitingCount } = filterMeetings(meetings, filters)

  const sourceKinds = useMemo(
    () => listMeetingSourceKinds(definitions.kinds, meetings),
    [ definitions.kinds, meetings ],
  )

  // None carries `matches`: the page applies them itself, so it knows when nothing matches.
  const filterDefinitions = useMemo((): SmartTableFilter<Meeting>[] => [
    {
      id: MEETING_FILTER_IDS.sources,
      label: t('table.source'),
      kind: 'multiSelect',
      options: sourceKinds.map((kind) => ({ value: kind, label: t(meetingSourceNameKey(kind)) })),
    },
    {
      id: MEETING_FILTER_IDS.states,
      label: t('table.state'),
      kind: 'multiSelect',
      options: MEETING_STATES.map((state) => ({ value: state, label: t(meetingStateLabelKeys[state]) })),
    },
    {
      id: MEETING_FILTER_IDS.project,
      label: t('filters.project'),
      kind: 'select',
      options: [
        { value: NO_PROJECT, label: t('filters.noProject') },
        ...projects.map((project) => ({ value: project.id, label: project.name })),
      ],
    },
    { id: MEETING_FILTER_IDS.waitingForReview, label: t('filters.waitingForReview'), kind: 'boolean' },
  ], [ t, sourceKinds, projects ])

  return <Page
    title={t('title')}
    parents={[{ label: t('knowledge:title'), href: UrlTree.knowledge }]}
    badges={waitingCount > 0 && <Chip size='sm' variant='soft' color='warning'>{
      t('list.waitingCount', { count: waitingCount, formattedCount: formatNumber(waitingCount) })
    }</Chip>}
    primaryAction={<Link to={UrlTree.meetingUploadNew} className={buttonVariants({ variant: 'primary' })}>
      <LuUpload className='size-4' aria-hidden />
      <span>{t('upload.action')}</span>
    </Link>}
    actions={[{
      id: 'sources',
      label: t('sources.title'),
      icon: LuSettings,
      href: UrlTree.settingsMeetings,
    }]}
  >
    <MeetingList
      status={status}
      meetings={matches}
      hasSources={hasSources}
      isFiltered={matches.length < meetings.length}
      filters={{
        definitions: filterDefinitions,
        values: toMeetingFilterValues(filters),
        onValuesChange: (values) => setSearchParams(
          writeMeetingFilters(fromMeetingFilterValues(values)),
          { replace: true },
        ),
      }}
    />
  </Page>
}
