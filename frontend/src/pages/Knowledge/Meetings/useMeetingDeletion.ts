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

// Deleting and restoring meetings, one or a selection, from the Meetings table, a row, or a
// meeting's own page (docs/meetings.md, Deleting). Deleting is soft, so it happens at once, with
// Undo in its toast restoring exactly what it deleted (`~/.claude/docs/ux.md`, Delete
// confirmations); after the toast, the Meetings page's Deleted filter and a deleted meeting's own
// page restore it. Each sends one change through the bulk route, in parts the API accepts, and
// files the meetings it answers in Redux (the events that follow are idempotent). A meeting the
// change did not apply to, changed meanwhile, is named in a warning that stays until dismissed.
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
    const titlesById = new Map(meetings.map((meeting) => [ meeting.id, titleOf(meeting) ]))
    const meetingIds = [ ...titlesById.keys() ]
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

    if (refused.length) {
      // Usually a meeting changed elsewhere since the list drew it, such as in another tab.
      console.debug('useMeetingDeletion: meetings refused the change', { refused, change })
      const [ first ] = refused
      const title = titlesById.get(first.meetingId) ?? first.meetingId
      const othersCount = refused.length - 1
      const message = othersCount
        ? t('delete.refusedMore', { title, count: othersCount })
        : t('delete.refusedOne', { title })
      toast.warning(message)
    }
    return changed
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
