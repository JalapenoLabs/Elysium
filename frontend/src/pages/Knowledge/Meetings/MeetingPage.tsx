// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectMeetingById } from '../../../store/meetingsSlice'

// User interface
import { Spinner } from '@heroui/react'
import { EmptyNotice } from '../../../components/EmptyNotice'
import { Page } from '../../../layout/page/Page'
import { MeetingReview } from './MeetingReview'

// Misc
import { useConnectorsLoader, useMeetingLoader } from '../../../hooks/useServerData'
import { UrlTree } from '../../../urls'

// `/knowledge/meetings/:meetingId`: one of the person's meetings as a page of the knowledge base,
// where they read its final transcript, see every mark on it, and correct it
// (docs/meeting-review.md, The meeting's page). Keyed by meeting, so nothing picked on one
// carries to the next.
export function MeetingPage() {
  const { meetingId = '' } = useParams()
  return <MeetingLoader key={meetingId} meetingId={meetingId} />
}

type Props = {
  meetingId: string
}

function MeetingLoader(props: Props) {
  const { t } = useTranslation([ 'meetings', 'knowledge' ])
  const status = useMeetingLoader(props.meetingId)
  useConnectorsLoader()
  const meeting = useAppSelector((state) => selectMeetingById(state, props.meetingId))
  // A meeting's home is the knowledge base's Meetings page, wherever else the tree lists it.
  const parents = [
    { label: t('knowledge:title'), href: UrlTree.knowledge },
    { label: t('title'), href: UrlTree.knowledgeMeetings },
  ]

  if (!meeting && status === 'loading') {
    return <div className='grid place-items-center py-10'><Spinner /></div>
  }
  if (!meeting) {
    return <Page title={t('page.missingTitle')} parents={parents}>
      <EmptyNotice>{t('page.missing')}</EmptyNotice>
    </Page>
  }
  return <MeetingReview meeting={meeting} parents={parents} />
}
