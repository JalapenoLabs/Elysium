// Copyright © 2026 Jalapeno Labs

import type { MeetingChangeResponse, MeetingTranscriptChange } from '../../../api/routes/meetingRoutes'

// Core
import { useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { mutate } from 'swr'

// Misc
import { getApiErrorCode, getApiErrorMessage } from '../../../api/errors'
import {
  editMeetingWords,
  meetingHistoryKey,
  undoMeetingChange,
} from '../../../api/routes/meetingRoutes'
import { toast } from '../../../toast/toast'

type Options = {
  meetingId: string
  // Applies what a change answered to the transcript the page holds.
  applyChange: (change: MeetingTranscriptChange) => void
  // Reads the transcript again, when a change found it behind.
  reload: () => void
}

// Every change a meeting's page makes. Each applies at once and says so in a toast with Undo, as
// attributing a record does (docs/meeting-review.md, Everything is an action); there is no Save.
// The page keeps the changes it made that still stand, newest last, so Ctrl+Z undoes the last of
// them (`undoLast`). An undo is not one of them: undoing again goes further back, never forward.
export function useMeetingChanges(options: Options) {
  const { t } = useTranslation([ 'meetings', 'common' ])
  const madeHere = useRef<string[]>([])

  function settle(response: MeetingChangeResponse) {
    options.applyChange(response.change)
    void mutate(meetingHistoryKey(options.meetingId))
  }

  async function undo(entryId: string) {
    try {
      const response = await undoMeetingChange(options.meetingId, entryId)
      madeHere.current = madeHere.current.filter((id) => id !== entryId)
      settle(response)
      toast.success(t('review.undone'))
    }
    catch (error) {
      console.debug('useMeetingChanges could not undo a change', { error, entryId })
      if (getApiErrorCode(error) === 'meeting_changed_since') {
        madeHere.current = madeHere.current.filter((id) => id !== entryId)
        toast.warning(t('review.changedSince'))
        return
      }
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
  }

  // Replaces the words from `startWordId` to `endWordId` with `text`. Resolves whether it did.
  async function edit(startWordId: string, endWordId: string, text: string) {
    try {
      const response = await editMeetingWords(options.meetingId, { startWordId, endWordId, text })
      madeHere.current.push(response.entry.id)
      settle(response)
      toast.success(t('review.edited'), {
        action: {
          label: t('review.undo'),
          onPress: () => void undo(response.entry.id),
        },
      })
      return true
    }
    catch (error) {
      console.debug('useMeetingChanges could not edit words', { error, startWordId, endWordId })
      // Someone changed those words meanwhile, so the page reads the transcript again and shows
      // them as they stand; the words picked are gone, and the editor with them.
      if (getApiErrorCode(error) === 'meeting_words_changed') {
        options.reload()
        toast.warning(t('review.wordsChanged'))
        return false
      }
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
      return false
    }
  }

  async function undoLast() {
    const last = madeHere.current.at(-1)
    if (!last) {
      toast(t('review.nothingToUndo'))
      return
    }
    await undo(last)
  }

  return { edit, undo, undoLast } as const
}

export type MeetingChanges = ReturnType<typeof useMeetingChanges>
