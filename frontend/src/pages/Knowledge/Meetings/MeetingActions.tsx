// Copyright © 2026 Jalapeno Labs

import type { Meeting } from '../../../api/routes/meetingRoutes'
import type { PageAction } from '../../../layout/page/pageActions'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsCompanyMember, selectMe } from '../../../store/authSlice'
import { selectConnectorById } from '../../../store/connectorsSlice'
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { meetingUpserted } from '../../../store/meetingsSlice'

// User interface
import { Button, Chip } from '@heroui/react'
import { LuCheck, LuHand, LuPlay, LuUpload, LuWorkflow } from 'react-icons/lu'
import { PageActions } from '../../../layout/page/PageActions'

// Misc
import { getApiErrorCode, getApiErrorMessage } from '../../../api/errors'
import { approveMeetingCorrection, setMeetingHold } from '../../../api/routes/meetingRoutes'
import { WAIT_FOR_APPROVAL_FIELD } from '../../../constants'
import { DateText } from '../../../format/DateText'
import { useUserNames } from '../../../hooks/useUserNames'
import { toast } from '../../../toast/toast'
import { getMeetingCaptureUploadUrl, getRunViewUrl } from '../../../urls'
import {
  correctionStatusChipColors,
  correctionStatusLabelKeys,
  correctionStatusOf,
} from '../../Settings/Meetings/meetingsPresentation'

type Props = {
  meeting: Meeting
}

// The meeting's actions, at the foot of its options: where its correction stands, Approve while
// the corrected transcript waits for it, holding it for approval by hand until it is corrected
// (unless its source already waits), its run, and adding a recording or transcript as another
// capture. Its processing is its owner's alone, so only they hold, approve, and add a capture
// (`crate::meetings::access`); a company member opens its run, as anyone of them opens any run.
// Captures' offsets and Done reviewing join them as they are built (docs/meeting-review.md,
// Roadmap).
export function MeetingActions(props: Props) {
  const { t } = useTranslation([ 'meetings', 'connectors', 'common' ])
  const dispatch = useAppDispatch()
  const userNames = useUserNames()
  const meeting = props.meeting
  const source = useAppSelector((state) => meeting.connectorId
    ? selectConnectorById(state, meeting.connectorId)
    : undefined)
  const [ acting, setActing ] = useState<'approve' | 'hold' | null>(null)
  const isOwner = useAppSelector(selectMe)?.user.id === meeting.ownerId
  const isCompanyMember = useAppSelector(selectIsCompanyMember)

  const correctionStatus = correctionStatusOf(meeting)
  const sourceWaits = source?.content[WAIT_FOR_APPROVAL_FIELD] === true
  const canApprove = isOwner && Boolean(meeting.correction && !meeting.correction.approvedAt)
  const approvedBy = meeting.correction?.approvedBy

  async function approve() {
    setActing('approve')
    try {
      const response = await approveMeetingCorrection(meeting.id)
      dispatch(meetingUpserted(response.meeting))
      toast.success(t('page.approved'))
    }
    catch (error) {
      console.debug('MeetingActions could not approve a correction', { error, meetingId: meeting.id })
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
      console.debug('MeetingActions could not change a hold', { error, meetingId: meeting.id, hold })
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
  if (isOwner && !meeting.correction && !sourceWaits) {
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
  if (meeting.runId && isCompanyMember) {
    actions.push({
      id: 'run',
      label: t('page.openRun'),
      icon: LuWorkflow,
      href: getRunViewUrl(meeting.runId),
    })
  }
  // Always offered its owner: an upload sent while the meeting's run is still going is refused,
  // saying so.
  if (isOwner) {
    actions.push({
      id: 'addCapture',
      label: t('upload.addCapture'),
      icon: LuUpload,
      href: getMeetingCaptureUploadUrl(meeting.id),
    })
  }

  // Before it is corrected, whether its run will wait, and why.
  let waitsNote: string | null = null
  if (!meeting.correction && sourceWaits) {
    waitsNote = t('correction.sourceWaits')
  }
  else if (!meeting.correction && meeting.holdForApproval) {
    waitsNote = t('correction.heldByHand')
  }

  return <div className='flex flex-col gap-2'>
    <div className='flex flex-wrap items-center gap-2'>
      <Chip size='sm' variant='soft' color={correctionStatusChipColors[correctionStatus]}>{
        t(correctionStatusLabelKeys[correctionStatus])
      }</Chip>
      {meeting.correction?.approvedAt && <span className='text-xs opacity-70'>
        {t('correction.approvedBy', { who: (approvedBy && userNames[approvedBy]) ?? t('common:table.someone') })}
        {' · '}
        <DateText value={meeting.correction.approvedAt} />
      </span>}
    </div>
    {waitsNote && <p className='text-xs opacity-70'>{waitsNote}</p>}
    <div className='flex flex-wrap items-center gap-2'>
      {canApprove && <Button
        size='sm'
        variant='primary'
        isPending={acting === 'approve'}
        isDisabled={acting !== null}
        onPress={() => void approve()}
      >
        <LuCheck className='size-4' aria-hidden />
        <span>{t('page.approve')}</span>
      </Button>}
      <PageActions actions={actions} />
    </div>
  </div>
}
