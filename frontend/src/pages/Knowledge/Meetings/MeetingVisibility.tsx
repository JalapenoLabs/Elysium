// Copyright © 2026 Jalapeno Labs

import type { Meeting, MeetingVisibilityResponse } from '../../../api/routes/meetingRoutes'
import type { AddPickerOption } from '../../../components/AddPicker'

// Core
import { useId } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { meetingUpserted } from '../../../store/meetingsSlice'
import { selectAllUsers, selectUserEntities } from '../../../store/usersSlice'

// User interface
import { Button, Chip, Tooltip } from '@heroui/react'
import { LuGlobe, LuLock, LuX } from 'react-icons/lu'
import { AddPicker } from '../../../components/AddPicker'

// Misc
import { getApiErrorCode, getApiErrorMessage } from '../../../api/errors'
import { setMeetingShared, setMeetingVisibility, undoMeetingChange } from '../../../api/routes/meetingRoutes'
import { usePendingAction } from '../../../hooks/usePendingAction'
import { useUsersLoader } from '../../../hooks/useServerData'
import { toast } from '../../../toast/toast'
import { correctionDecisionOf, visibilityChipColors, visibilityIcons, visibilityLabelKeys } from './meetingVisibility'

type Props = {
  meeting: Meeting
}

// Who reads the meeting, among its options (docs/meeting-review.md, Who sees a meeting): open to
// everyone, private to its owner and the people it is shared with, or new and private until its
// correction decides, with what the correction said when it kept it private. Its owner and admins
// make it private, open it, and share it, each at once with Undo in its toast, while it is not
// deleted, since a deleted meeting changes no further until it is restored; everyone else reads
// who it is shared with.
export function MeetingVisibility(props: Props) {
  const { t } = useTranslation([ 'meetings', 'common' ])
  const { meeting } = props
  const dispatch = useAppDispatch()
  useUsersLoader()
  const users = useAppSelector(selectAllUsers)
  const usersById = useAppSelector(selectUserEntities)
  const pending = usePendingAction<string>()
  // The lane renders twice on a narrow screen (beside the transcript, hidden, and in its drawer), so
  // the picker's id is generated rather than fixed, keeping ids unique on the page.
  const pickerId = useId()

  const visibility = meeting.visibility
  const Icon = visibilityIcons[visibility]
  const decision = correctionDecisionOf(meeting.visibilityChange)
  const ownerName = usersById[meeting.ownerId]?.name ?? t('common:table.someone')
  const nameOf = (personId: string) => usersById[personId]?.name ?? t('common:table.someone')
  const changesWhoReads = meeting.canManage && !meeting.deletedAt

  const shareable: AddPickerOption[] = []
  for (const user of users) {
    const isPerson = user.kind === 'person' && user.status === 'active'
    if (isPerson && user.id !== meeting.ownerId && !meeting.sharedWith.includes(user.id)) {
      shareable.push({ id: user.id, label: user.name, detail: user.email ?? undefined })
    }
  }

  // Applies a change made here, and says so with Undo, which puts it back as its history does.
  async function change(request: Promise<MeetingVisibilityResponse>, confirmation: string) {
    try {
      const response = await request
      dispatch(meetingUpserted(response.meeting))
      toast.success(confirmation, {
        action: {
          label: t('review.undo'),
          onPress: () => void undo(response.entry.id),
        },
      })
    }
    catch (error) {
      console.debug('MeetingVisibility could not change who reads a meeting', { error, meetingId: meeting.id })
      // Someone else made the same change meanwhile, which the meeting shows once it reloads.
      if (getApiErrorCode(error) === 'meeting_visibility_unchanged') {
        toast.warning(t('visibility.alreadySo'))
        return
      }
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
  }

  async function undo(entryId: string) {
    try {
      await undoMeetingChange(meeting.id, entryId)
      toast.success(t('review.undone'))
    }
    catch (error) {
      console.debug('MeetingVisibility could not undo a change', { error, entryId })
      if (getApiErrorCode(error) === 'meeting_changed_since') {
        toast.warning(t('review.changedSince'))
        return
      }
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
  }

  const opens = visibility !== 'public'
  const toggleLabel = opens
    ? t('visibility.open')
    : t('visibility.makePrivate')

  return <div>
    <p className='compact text-xs font-semibold uppercase opacity-60'>{t('visibility.heading')}</p>
    <div className='compact flex flex-wrap items-center gap-2'>
      <Chip size='sm' variant='soft' color={visibilityChipColors[visibility]}>
        <Icon className='size-3' aria-hidden />
        <Chip.Label>{t(visibilityLabelKeys[visibility])}</Chip.Label>
      </Chip>
      <span className='text-xs opacity-70'>{t('visibility.owner', { owner: ownerName })}</span>
    </div>
    {decision && <p className='compact text-sm opacity-80'>{
      t(decision.key, { why: decision.why ?? '' })
    }</p>}

    {Boolean(meeting.sharedWith.length) && <ul
      aria-label={t('visibility.sharedWith')}
      className='compact flex flex-wrap items-center gap-1.5'
    >{
      meeting.sharedWith.map((personId) => {
        const name = nameOf(personId)
        const stopLabel = t('visibility.stopSharing', { person: name })
        return <li key={personId} className='flex items-center'>
          <Chip size='sm' variant='soft'>
            <Chip.Label>{name}</Chip.Label>
          </Chip>
          {changesWhoReads && <Tooltip delay={400}>
            <Button
              isIconOnly
              size='sm'
              variant='ghost'
              className='size-5 min-w-0'
              aria-label={stopLabel}
              isPending={pending.pendingKey === personId}
              isDisabled={pending.isOtherPending(personId)}
              onPress={() => void pending.run(personId, () => change(
                setMeetingShared(meeting.id, personId, false),
                t('visibility.stoppedSharing', { person: name }),
              ))}
            >
              <LuX className='size-3' aria-hidden />
            </Button>
            <Tooltip.Content>{stopLabel}</Tooltip.Content>
          </Tooltip>}
        </li>
      })
    }</ul>}

    {changesWhoReads && <div className='flex flex-wrap items-center gap-2'>
      <Button
        size='sm'
        variant='outline'
        isPending={pending.pendingKey === 'visibility'}
        isDisabled={pending.isOtherPending('visibility')}
        onPress={() => void pending.run('visibility', () => change(
          setMeetingVisibility(meeting.id, opens
            ? 'public'
            : 'private'),
          opens
            ? t('visibility.opened')
            : t('visibility.madePrivate'),
        ))}
      >
        {opens
          ? <LuGlobe className='size-4' aria-hidden />
          : <LuLock className='size-4' aria-hidden />}
        <span>{toggleLabel}</span>
      </Button>
      <AddPicker
        id={pickerId}
        className='w-44'
        label={t('visibility.share')}
        emptyLabel={t('visibility.nobodyToShareWith')}
        options={shareable}
        isDisabled={pending.isAnyPending}
        onPick={(personId) => void pending.run(personId, () => change(
          setMeetingShared(meeting.id, personId, true),
          t('visibility.shared', { person: nameOf(personId) }),
        ))}
      />
    </div>}
  </div>
}
