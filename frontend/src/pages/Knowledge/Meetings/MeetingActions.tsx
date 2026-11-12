// Copyright © 2026 Jalapeno Labs

import type { Meeting } from '../../../api/routes/meetingRoutes'
import type { PageAction } from '../../../layout/page/pageActions'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { selectConnectorById } from '../../../store/connectorsSlice'
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { meetingUpserted } from '../../../store/meetingsSlice'

// User interface
import { Button, Chip } from '@heroui/react'
import { LuCheck, LuCheckCheck, LuHand, LuPlay, LuTrash2, LuUpload, LuWorkflow } from 'react-icons/lu'
import { PageActions } from '../../../layout/page/PageActions'

// Misc
import { getApiErrorCode, getApiErrorMessage } from '../../../api/errors'
import { approveMeetingCorrection, markMeetingReviewed, setMeetingHold } from '../../../api/routes/meetingRoutes'
import { WAIT_FOR_APPROVAL_FIELD } from '../../../constants'
import { DateText } from '../../../format/DateText'
import { useUserNames } from '../../../hooks/useUserNames'
import { toast } from '../../../toast/toast'
import { getMeetingCaptureUploadUrl, getRunViewUrl, UrlTree } from '../../../urls'
import {
  correctionStatusChipColors,
  correctionStatusLabelKeys,
  correctionStatusOf,
} from '../../Settings/Meetings/meetingsPresentation'
import { useMeetingDeletion } from './useMeetingDeletion'

type Props = {
  meeting: Meeting
}

// The meeting's actions, at the foot of its options: where its correction stands, Approve while
// the corrected transcript waits for it, Done reviewing once it was segmented, which segments it
// again from its corrected words, holding it for approval by hand until it is corrected (unless
// its source already waits), its run, adding a recording or transcript as another capture, and
// Delete, last. A meeting merged into another offers its run alone, and says why, as does one
// deleted, which its notice restores (`DeletedMeetingNotice`). Privacy and sharing join them as
// they are built (docs/meeting-review.md, Roadmap).
export function MeetingActions(props: Props) {
  const { t } = useTranslation([ 'meetings', 'connectors', 'common' ])
  const dispatch = useAppDispatch()
  const userNames = useUserNames()
  const deletion = useMeetingDeletion()
  const meeting = props.meeting
  const source = useAppSelector((state) => meeting.connectorId
    ? selectConnectorById(state, meeting.connectorId)
    : undefined)
  const [ acting, setActing ] = useState<'approve' | 'hold' | 'reviewed' | 'delete' | null>(null)

  const correctionStatus = correctionStatusOf(meeting)
  const sourceWaits = source?.content[WAIT_FOR_APPROVAL_FIELD] === true
  // A meeting merged into another changes there: it holds no capture, its run goes no further,
  // and an upload into it is refused, so it offers to approve, hold, review, add, or delete nothing;
  // the meeting it went into is the one to delete. A deleted one changes no further until it is
  // restored.
  const isMergedAway = Boolean(meeting.mergedInto)
  const isDeleted = Boolean(meeting.deletedAt)
  const isOwn = !isMergedAway && !isDeleted
  const canApprove = isOwn && Boolean(meeting.correction && !meeting.correction.approvedAt)
  const approvedBy = meeting.correction?.approvedBy
  // Segmented once, and no run of it going, held, or failed.
  const canFinishReview = isOwn && meeting.state === 'done'

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

  async function finishReview() {
    setActing('reviewed')
    try {
      const response = await markMeetingReviewed(meeting.id)
      dispatch(meetingUpserted(response.meeting))
      toast.success(t('page.reviewDone'))
    }
    catch (error) {
      console.debug('MeetingActions could not finish a review', { error, meetingId: meeting.id })
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

  // Soft, so at once, with Undo in its toast, which comes back here (`useMeetingDeletion`).
  async function remove() {
    setActing('delete')
    const wasDeleted = await deletion.remove([ meeting ], UrlTree.knowledgeMeetings)
    // Gone from here on success; stays to be tried again otherwise.
    if (!wasDeleted) {
      setActing(null)
    }
  }

  const actions: PageAction[] = []
  if (isOwn && !meeting.correction && !sourceWaits) {
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
  // Offered while the meeting is its own: an upload sent while its run is still going is refused,
  // saying so.
  if (isOwn) {
    actions.push({
      id: 'addCapture',
      label: t('upload.addCapture'),
      icon: LuUpload,
      href: getMeetingCaptureUploadUrl(meeting.id),
    })
    actions.push({
      id: 'delete',
      label: t('common:actions.delete'),
      icon: LuTrash2,
      isDanger: true,
      isPending: acting === 'delete',
      isDisabled: acting !== null,
      onAction: () => void remove(),
    })
  }

  // Why nothing changes here, or before it is corrected, whether its run will wait, and why.
  let waitsNote: string | null = null
  if (isMergedAway) {
    waitsNote = t('page.mergedReadOnly')
  }
  else if (isDeleted) {
    waitsNote = t('delete.readOnly')
  }
  else if (!meeting.correction && sourceWaits) {
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
      {canFinishReview && <Button
        size='sm'
        variant='primary'
        isPending={acting === 'reviewed'}
        isDisabled={acting !== null}
        onPress={() => void finishReview()}
      >
        <LuCheckCheck className='size-4' aria-hidden />
        <span>{t('page.doneReviewing')}</span>
      </Button>}
      <PageActions actions={actions} />
    </div>
  </div>
}
