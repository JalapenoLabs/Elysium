// Copyright © 2026 Jalapeno Labs

import type { Meeting } from '../../api/routes/meetingRoutes'
import type { PageCrumb } from '../../layout/page/PageHeader'

// Core
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'
import useSWR from 'swr'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectMeetingById } from '../../store/meetingsSlice'

// User interface
import { Spinner } from '@heroui/react'
import { EmptyNotice } from '../../components/EmptyNotice'
import { SettingsPage } from '../Settings/SettingsPage'
import { UploadMeetingForm } from './UploadMeetingForm'

// Misc
import { getMeetingUploadPolicy, listMeetingUploads } from '../../api/routes/meetingUploadRoutes'
import { useMeetingLoader, useStorageLocationsLoader } from '../../hooks/useServerData'
import { UrlTree, getMeetingUrl } from '../../urls'

// `/settings/meetings/new` uploads a meeting: a recording, a transcript, or both. At
// `/settings/meetings/:meetingId/captures/new` the same files become one more capture of a
// meeting the person has. See docs/meetings.md, Uploads.
export function UploadMeetingPage() {
  const { meetingId } = useParams()
  if (meetingId) {
    return <CaptureUpload key={meetingId} meetingId={meetingId} />
  }
  return <UploadPanel meeting={null} />
}

type CaptureProps = {
  meetingId: string
}

// A capture's upload, once its meeting is loaded, since the page names it.
function CaptureUpload(props: CaptureProps) {
  const { t } = useTranslation('meetings')
  const status = useMeetingLoader(props.meetingId)
  const meeting = useAppSelector((state) => selectMeetingById(state, props.meetingId))

  if (!meeting && status === 'loading') {
    return <div className='grid place-items-center py-10'><Spinner /></div>
  }
  if (!meeting) {
    return <SettingsPage
      title={t('page.missingTitle')}
      parents={[{ label: t('title'), href: UrlTree.settingsMeetings }]}
    >
      <EmptyNotice>{t('page.missing')}</EmptyNotice>
    </SettingsPage>
  }
  return <UploadPanel meeting={meeting} />
}

type PanelProps = {
  // The meeting a capture is added to, or null for a new meeting.
  meeting: Meeting | null
}

// The page around the form: what a meeting takes, and the uploads to resume, read before the
// form opens so it starts from them.
function UploadPanel(props: PanelProps) {
  const { t } = useTranslation([ 'meetings', 'common' ])
  const meetingId = props.meeting?.id ?? null
  useStorageLocationsLoader()
  const policy = useSWR([ 'v1/meeting-uploads/policy', meetingId ], () => getMeetingUploadPolicy(meetingId))
  const unfinished = useSWR('v1/meeting-uploads', listMeetingUploads)

  const parents: PageCrumb[] = [{ label: t('title'), href: UrlTree.settingsMeetings }]
  if (props.meeting) {
    parents.push({
      label: props.meeting.title || t('list.untitled'),
      href: getMeetingUrl(props.meeting.id),
    })
  }
  const title = props.meeting
    ? t('upload.captureTitle')
    : t('upload.title')
  const hasFailed = Boolean(policy.error || unfinished.error)
  const isLoaded = Boolean(policy.data && unfinished.data)

  return <SettingsPage title={title} parents={parents} help={t('upload.help')}>
    {!isLoaded && !hasFailed && <div className='grid place-items-center py-16'>
      <Spinner />
    </div>}
    {hasFailed && <p className='py-10 text-center text-sm text-danger'>{t('common:errors.unexpected')}</p>}
    {policy.data && unfinished.data && !hasFailed && <UploadMeetingForm
      meeting={props.meeting}
      policy={policy.data.policy}
      unfinished={unfinished.data.uploads}
    />}
  </SettingsPage>
}
