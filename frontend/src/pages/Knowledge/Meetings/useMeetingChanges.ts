// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type {
  MeetingChangeResponse,
  MeetingHistoryEntry,
  MeetingTranscriptChange,
} from '../../../api/routes/meetingRoutes'

// Core
import { useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { mutate } from 'swr'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { meetingUpserted } from '../../../store/meetingsSlice'

// Misc
import { getApiErrorCode, getApiErrorMessage } from '../../../api/errors'
import {
  editMeetingWords,
  meetingHistoryKey,
  setCaptureOffset,
  splitCapture,
  undoMeetingChange,
} from '../../../api/routes/meetingRoutes'
import { trackMeetingChange } from '../../../realtime/meetingChangeEchoes'
import { toast } from '../../../toast/toast'

// What a split refused says, by the API's code: the capture went back already, or a capture the
// merge brought was moved by hand since, which splitting would overwrite.
const splitRefusalKeys = {
  capture_not_merged: 'review.captures.notMerged',
  meeting_changed: 'review.captures.movedSince',
} as const satisfies Record<string, ParseKeys<'meetings'>>

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
  const dispatch = useAppDispatch()
  const madeHere = useRef<string[]>([])

  // Its own change applies from what the route answered; only the history, small, is read again.
  // The change's own announcement is then no news to this page (`meetingChangeEchoes`).
  function settle(response: MeetingChangeResponse) {
    options.applyChange(response.change)
    void mutate(meetingHistoryKey(options.meetingId))
  }

  async function undo(entryId: string) {
    try {
      const response = await trackMeetingChange(
        options.meetingId,
        undoMeetingChange(options.meetingId, entryId),
      )
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
      const response = await trackMeetingChange(
        options.meetingId,
        editMeetingWords(options.meetingId, { startWordId, endWordId, text }),
      )
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

  // A change that moved a capture lines the whole transcript up again, which no change of words
  // describes, so its announcement names no entry and every page holding the meeting reads it
  // again, this one included (`announce_lined_up` in api/src/routes/v1/meetings/mod.rs). It still
  // joins the changes Ctrl+Z undoes, and says so with Undo.
  function linedUp(entry: MeetingHistoryEntry, confirmation: string) {
    madeHere.current.push(entry.id)
    toast.success(confirmation, {
      action: {
        label: t('review.undo'),
        onPress: () => void undo(entry.id),
      },
    })
  }

  // Sets where capture `captureId` starts, `offsetMs` into the meeting, by hand. Resolves whether
  // it did; one refused keeps the editor open with what was typed.
  async function moveCapture(captureId: string, offsetMs: number) {
    try {
      const response = await setCaptureOffset(options.meetingId, captureId, offsetMs)
      linedUp(response.entry, t('review.captures.moved'))
      return true
    }
    catch (error) {
      console.debug('useMeetingChanges could not move a capture', { error, captureId, offsetMs })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
      return false
    }
  }

  // Splits the captures the merge that brought `captureId` brought back out into their own
  // meeting. Its Undo merges them again.
  async function splitOut(captureId: string) {
    try {
      const response = await splitCapture(options.meetingId, captureId)
      dispatch(meetingUpserted(response.meeting))
      linedUp(response.entry, t('review.captures.splitDone'))
    }
    catch (error) {
      console.debug('useMeetingChanges could not split a capture out', { error, captureId })
      const code = getApiErrorCode(error)
      // The page was behind: it reads the meeting again and shows its captures as they are.
      if (code === 'capture_not_merged' || code === 'meeting_changed') {
        options.reload()
        toast.warning(t(splitRefusalKeys[code]))
        return
      }
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
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

  return { edit, moveCapture, splitOut, undo, undoLast } as const
}

export type MeetingChanges = ReturnType<typeof useMeetingChanges>
