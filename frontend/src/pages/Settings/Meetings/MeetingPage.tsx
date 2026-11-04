// Copyright © 2026 Jalapeno Labs

import type { Meeting } from '../../../api/routes/meetingRoutes'
import type { PageAction } from '../../../layout/page/pageActions'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'

// Redux
import { selectConnectorById } from '../../../store/connectorsSlice'
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { meetingUpserted, selectMeetingById } from '../../../store/meetingsSlice'

// User interface
import { Button, Chip, Link, Spinner, Tabs } from '@heroui/react'
import { LuCheck, LuExternalLink, LuHand, LuPlay, LuWorkflow } from 'react-icons/lu'
import { PageTabs } from '../../../components/PageTabs'
import { ConnectorLogo } from '../Connectors/ConnectorLogo'
import { SettingsPage } from '../SettingsPage'
import { MeetingTopics } from './MeetingTopics'
import { MeetingTranscript } from './MeetingTranscript'

// Misc
import { getApiErrorCode, getApiErrorMessage } from '../../../api/errors'
import { approveMeetingCorrection, setMeetingHold } from '../../../api/routes/meetingRoutes'
import { DateText } from '../../../format/DateText'
import { formatDuration } from '../../../format/durations'
import { useConnectorsLoader, useMeetingLoader, useMeetingTopicsLoader } from '../../../hooks/useServerData'
import { useTabParam } from '../../../hooks/useTabParam'
import { useUserNames } from '../../../hooks/useUserNames'
import { UrlTree, getRunViewUrl } from '../../../urls'
import { toast } from '../../../toast/toast'
import {
  correctionStatusChipColors,
  correctionStatusLabelKeys,
  correctionStatusOf,
  meetingStateChipColors,
  meetingStateLabelKeys,
} from './meetingsPresentation'
import { WAIT_FOR_APPROVAL_FIELD } from '../../../constants'

// The meeting's lists, one per tab: its topics, first, and its transcript.
const TABS = [ 'topics', 'transcript' ] as const

// `/settings/meetings/:meetingId`: one of the person's meetings, its topics with the projects
// each is on, its transcript as heard beside its corrections, and Approve. Until it is corrected
// the person can hold it for their approval by hand, unless its source already waits for it. See
// docs/meetings.md, Correction, and In the web app.
export function MeetingPage() {
  const { t } = useTranslation([ 'meetings', 'connectors', 'common' ])
  const { meetingId = '' } = useParams()
  const status = useMeetingLoader(meetingId)
  useConnectorsLoader()
  const meeting = useAppSelector((state) => selectMeetingById(state, meetingId))
  const parents = [{ label: t('title'), href: UrlTree.settingsMeetings }]

  if (!meeting && status === 'loading') {
    return <div className='grid place-items-center py-10'><Spinner /></div>
  }
  if (!meeting) {
    return <SettingsPage title={t('page.missingTitle')} parents={parents}>
      <p className='opacity-70'>{t('page.missing')}</p>
    </SettingsPage>
  }
  return <MeetingView meeting={meeting} />
}

type Props = {
  meeting: Meeting
}

// A meeting that loaded, so everything below can rely on it.
function MeetingView(props: Props) {
  const { t } = useTranslation([ 'meetings', 'connectors', 'common' ])
  const dispatch = useAppDispatch()
  const userNames = useUserNames()
  const meeting = props.meeting
  const source = useAppSelector((state) => meeting.connectorId
    ? selectConnectorById(state, meeting.connectorId)
    : undefined)
  const [ acting, setActing ] = useState<'approve' | 'hold' | null>(null)
  const [ selectedTab, selectTab ] = useTabParam(TABS)
  const topics = useMeetingTopicsLoader(meeting.id)

  const correctionStatus = correctionStatusOf(meeting)
  const sourceWaits = source?.content[WAIT_FOR_APPROVAL_FIELD] === true
  const canApprove = Boolean(meeting.correction && !meeting.correction.approvedAt)

  async function approve() {
    setActing('approve')
    try {
      const response = await approveMeetingCorrection(meeting.id)
      dispatch(meetingUpserted(response.meeting))
      toast.success(t('page.approved'))
    }
    catch (error) {
      console.debug('MeetingPage could not approve a correction', { error, meetingId: meeting.id })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
    finally {
      setActing(null)
    }
  }

  async function toggleHold() {
    setActing('hold')
    const hold = !meeting.holdForApproval
    try {
      const response = await setMeetingHold(meeting.id, hold)
      dispatch(meetingUpserted(response.meeting))
      toast.success(hold
        ? t('page.held')
        : t('page.released'))
    }
    catch (error) {
      console.debug('MeetingPage could not change a hold', { error, meetingId: meeting.id, hold })
      // Corrected meanwhile: approving is what remains, and the page shows it once it reloads.
      if (getApiErrorCode(error) === 'meeting_corrected') {
        toast.warning(t('page.correctedMeanwhile'))
        return
      }
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
    finally {
      setActing(null)
    }
  }

  const actions: PageAction[] = []
  if (!meeting.correction && !sourceWaits) {
    actions.push({
      id: 'hold',
      label: meeting.holdForApproval
        ? t('page.release')
        : t('page.hold'),
      icon: meeting.holdForApproval
        ? LuPlay
        : LuHand,
      isPending: acting === 'hold',
      isDisabled: acting !== null,
      onAction: () => void toggleHold(),
    })
  }
  if (meeting.runId) {
    actions.push({
      id: 'run',
      label: t('page.openRun'),
      icon: LuWorkflow,
      href: getRunViewUrl(meeting.runId),
    })
  }

  const approvedBy = meeting.correction?.approvedBy
  // Before it is corrected, whether its run will wait, and why.
  let waitsNote: string | null = null
  if (!meeting.correction && sourceWaits) {
    waitsNote = t('correction.sourceWaits')
  }
  else if (!meeting.correction && meeting.holdForApproval) {
    waitsNote = t('correction.heldByHand')
  }

  return <SettingsPage
    title={meeting.title || t('list.untitled')}
    icon={<ConnectorLogo kind={meeting.sourceKind} className='size-8' />}
    parents={[{ label: t('title'), href: UrlTree.settingsMeetings }]}
    // While its run waits for the person, its processing waits too; the correction's status
    // below says so, rather than a state that reads as work under way.
    badges={!meeting.awaitingApproval && <Chip
      size='sm'
      variant='soft'
      color={meetingStateChipColors[meeting.state]}
    >{
        t(meetingStateLabelKeys[meeting.state])
      }</Chip>}
    primaryAction={canApprove && <Button
      variant='primary'
      isPending={acting === 'approve'}
      isDisabled={acting !== null}
      onPress={() => void approve()}
    >
      <LuCheck className='size-4' aria-hidden />
      <span>{t('page.approve')}</span>
    </Button>}
    actions={actions}
  >
    <div className='relaxed flex flex-wrap items-center gap-x-2 gap-y-1 text-sm'>
      <DateText value={meeting.startedAt} className='opacity-70' />
      {meeting.durationMs !== null && <>
        <span aria-hidden className='opacity-70'>·</span>
        <span className='opacity-70'>{formatDuration(meeting.durationMs)}</span>
      </>}
      {meeting.url && <>
        <span aria-hidden className='opacity-70'>·</span>
        <Link href={meeting.url} target='_blank' rel='noreferrer' className='text-link text-sm'>
          <span>{t('page.openAtSource', { source: t(`connectors:kinds.${meeting.sourceKind}.name`) })}</span>
          <LuExternalLink className='ml-1 inline size-3.5' aria-hidden />
        </Link>
      </>}
    </div>

    <div className='relaxed flex flex-wrap items-center gap-2'>
      <Chip size='sm' variant='soft' color={correctionStatusChipColors[correctionStatus]}>{
        t(correctionStatusLabelKeys[correctionStatus])
      }</Chip>
      {meeting.correction?.approvedAt && <span className='text-sm opacity-70'>
        {t('correction.approvedBy', { who: (approvedBy && userNames[approvedBy]) ?? t('common:table.someone') })}
        {' · '}
        <DateText value={meeting.correction.approvedAt} />
      </span>}
      {waitsNote && <span className='text-sm opacity-70'>{waitsNote}</span>}
    </div>

    <PageTabs
      label={t('page.tabs.label')}
      selectedKey={selectedTab}
      onSelectionChange={selectTab}
      tabs={[
        { id: 'topics', label: t('page.tabs.topics'), count: topics.total },
        { id: 'transcript', label: t('page.tabs.transcript'), count: null },
      ]}
    >
      <Tabs.Panel id='topics'>
        <MeetingTopics meeting={meeting} status={topics.status} />
      </Tabs.Panel>
      <Tabs.Panel id='transcript'>
        <MeetingTranscript meeting={meeting} />
      </Tabs.Panel>
    </PageTabs>
  </SettingsPage>
}
