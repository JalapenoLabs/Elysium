// Copyright © 2026 Jalapeno Labs

import type { Meeting, MeetingBulkChange, MeetingRefusal } from '../../../api/routes/meetingRoutes'

// Core
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { meetingsUpserted } from '../../../store/meetingsSlice'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { changeMeetings } from '../../../api/routes/meetingRoutes'
import { MEETING_BULK_MAX_MEETINGS } from '../../../constants'
import { toast } from '../../../toast/toast'
import { getMeetingUrl } from '../../../urls'

// The refusal a meeting answers to someone who reads it but may not delete or restore it.
const OWNER_OR_ADMIN = 'owner_or_admin'

// Deleting and restoring meetings, one or a selection, from the Meetings table, a row, or a
// meeting's own page (docs/meetings.md, Deleting). Deleting is soft, so it happens at once, with
// Undo in its toast restoring exactly what it deleted (`~/.claude/docs/ux.md`, Delete
// confirmations); after the toast, the Meetings page's Deleted filter and a deleted meeting's own
// page restore it. Each sends one change through the bulk route, in parts the API accepts, and
// files the meetings it answers in Redux (the events that follow are idempotent). Only its owner
// and admins delete or restore a meeting (`canManage`), so one of a selection they may not is
// left out of the request; it, and one the change did not apply to because it changed meanwhile,
// is named in a warning that stays until dismissed.
export function useMeetingDeletion() {
  const { t } = useTranslation([ 'meetings', 'common' ])
  const dispatch = useAppDispatch()
  const navigate = useNavigate()

  function titleOf(meeting: Meeting) {
    return meeting.title || t('list.untitled')
  }

  // Resolves to the meetings changed, or null when a request failed; either way the person has
  // been told of anything refused. Nothing is filed in Redux here, so a caller leaving a meeting's
  // page can go before the page draws it deleted.
  async function send(meetings: Meeting[], change: MeetingBulkChange) {
    const titlesById = new Map<string, string>()
    const meetingIds: string[] = []
    const notTheirs: string[] = []
    for (const meeting of meetings) {
      titlesById.set(meeting.id, titleOf(meeting))
      if (meeting.canManage) {
        meetingIds.push(meeting.id)
      }
      else {
        notTheirs.push(meeting.id)
      }
    }
    const changed: Meeting[] = []
    const refused: MeetingRefusal[] = []
    try {
      for (let start = 0; start < meetingIds.length; start += MEETING_BULK_MAX_MEETINGS) {
        const part = meetingIds.slice(start, start + MEETING_BULK_MAX_MEETINGS)
        const response = await changeMeetings(part, change)
        changed.push(...response.meetings)
        refused.push(...response.refused)
      }
    }
    catch (error) {
      console.debug('useMeetingDeletion failed to change meetings', { error, change, meetingIds })
      // What a part already changed stays changed; its events bring it to every list.
      dispatch(meetingsUpserted(changed))
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
      return null
    }

    // One the person may not change is theirs to know of, whether the list left it out or the API
    // refused it, a role changed meanwhile; any other was changed elsewhere since the list drew it,
    // such as in another tab.
    const changedMeanwhile: string[] = []
    for (const refusal of refused) {
      if (refusal.code === OWNER_OR_ADMIN) {
        notTheirs.push(refusal.meetingId)
      }
      else {
        changedMeanwhile.push(refusal.meetingId)
      }
    }
    if (refused.length) {
      console.debug('useMeetingDeletion: meetings refused the change', { refused, change })
    }
    warnOf(notTheirs, titlesById, 'delete.notTheirsOne', 'delete.notTheirsMore')
    warnOf(changedMeanwhile, titlesById, 'delete.refusedOne', 'delete.refusedMore')
    return changed
  }

  // Names the first of `meetingIds` and counts the rest in one warning, which stays until
  // dismissed; none, no warning.
  function warnOf(
    meetingIds: string[],
    titlesById: Map<string, string>,
    oneKey: 'delete.notTheirsOne' | 'delete.refusedOne',
    moreKey: 'delete.notTheirsMore' | 'delete.refusedMore',
  ) {
    const [ firstId ] = meetingIds
    if (!firstId) {
      return
    }
    const title = titlesById.get(firstId) ?? firstId
    const othersCount = meetingIds.length - 1
    const message = othersCount
      ? t(moreKey, { title, count: othersCount })
      : t(oneKey, { title })
    toast.warning(message)
  }

  // Deletes `meetings`, leaving for `leaveTo` first when one is given, as a meeting's own page
  // does. Resolves to whether any was deleted.
  async function remove(meetings: Meeting[], leaveTo?: string) {
    const deleted = await send(meetings, 'delete')
    if (!deleted?.length) {
      return false
    }
    if (leaveTo) {
      void navigate(leaveTo)
    }
    dispatch(meetingsUpserted(deleted))
    toast.success(t('delete.deleted', { count: deleted.length, title: titleOf(deleted[0]) }), {
      action: {
        label: t('common:actions.undo'),
        onPress: () => void undoRemove(deleted, Boolean(leaveTo)),
      },
    })
    return true
  }

  // Undo of a deletion from a meeting's own page comes back to that page, wherever the person is
  // by then.
  async function undoRemove(meetings: Meeting[], isReturning: boolean) {
    const isRestored = await restore(meetings)
    if (isRestored && isReturning) {
      void navigate(getMeetingUrl(meetings[0].id))
    }
  }

  // Restores `meetings`, each with the excerpts its deletion took, and resolves to whether any was
  // restored.
  async function restore(meetings: Meeting[]) {
    const restored = await send(meetings, 'restore')
    if (!restored?.length) {
      return false
    }
    dispatch(meetingsUpserted(restored))
    toast.success(t('delete.restored', { count: restored.length, title: titleOf(restored[0]) }))
    return true
  }

  return { remove, restore } as const
}
