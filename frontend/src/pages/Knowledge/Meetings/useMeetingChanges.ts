// Copyright © 2026 Jalapeno Labs

import type { MeetingChangeResponse, MeetingTranscriptChange } from '../../../api/routes/meetingRoutes'

// Core
import { useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { mutate } from 'swr'

// Misc
import { getApiErrorCode, getApiErrorMessage } from '../../../api/errors'
import {
  assignMeetingExcerpt,
  blackOutMeetingWords,
  editMeetingWords,
  meetingHistoryKey,
  meetingThreadsKey,
  removeMeetingMark,
  removeMeetingThread,
  replyInMeetingThread,
  setMeetingThreadResolved,
  startMeetingThread,
  undoMeetingChange,
} from '../../../api/routes/meetingRoutes'
import { timelineKeyPrefix } from '../../../api/routes/timelineRoutes'
import { trackMeetingChange } from '../../../realtime/meetingChangeEchoes'
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

  // Its own change applies from what the route answered; only the history and the threads' comments,
  // both small, are read again, since a change to a thread, or an undo of one, changes which threads
  // stand. The change's own announcement is then no news to this page (`meetingChangeEchoes`).
  function settle(response: MeetingChangeResponse) {
    options.applyChange(response.change)
    void mutate(meetingHistoryKey(options.meetingId))
    void mutate(meetingThreadsKey(options.meetingId))
    // An excerpt made, removed, or put back is a record on its projects' timelines.
    if (typeof response.entry.data.recordId === 'string') {
      void mutate((key) => typeof key === 'string' && key.startsWith(timelineKeyPrefix))
    }
  }

  // Applies a change made here, and says so with Undo.
  function made(response: MeetingChangeResponse, confirmation: string) {
    madeHere.current.push(response.entry.id)
    settle(response)
    toast.success(confirmation, {
      action: {
        label: t('review.undo'),
        onPress: () => void undo(response.entry.id),
      },
    })
  }

  // Tells the person a change was refused: a thread removed meanwhile reads the threads again.
  function refused(error: unknown) {
    if (getApiErrorCode(error) === 'meeting_thread_removed') {
      void mutate(meetingThreadsKey(options.meetingId))
      toast.warning(t('review.comments.threadRemoved'))
      return
    }
    toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
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
      made(response, t('review.edited'))
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

  // Starts a thread on the words from `startWordId` to `endWordId`. Resolves the new thread's id,
  // or null when it was refused.
  async function startThread(startWordId: string, endWordId: string, body: string, mentions: string[]) {
    try {
      const response = await trackMeetingChange(
        options.meetingId,
        startMeetingThread(options.meetingId, { startWordId, endWordId, body, mentions }),
      )
      made(response, t('review.comments.started'))
      return response.comment.threadId
    }
    catch (error) {
      console.debug('useMeetingChanges could not start a thread', { error, startWordId, endWordId })
      if (getApiErrorCode(error) === 'meeting_words_changed') {
        options.reload()
        toast.warning(t('review.wordsChanged'))
        return null
      }
      refused(error)
      return null
    }
  }

  // Replies in thread `threadId`. A reply is a comment, not a change in the meeting's history, so it
  // has no Undo, and its announcement names no entry; it shows in its thread at once. Resolves
  // whether it was posted.
  async function reply(threadId: string, body: string, mentions: string[]) {
    try {
      await replyInMeetingThread(options.meetingId, threadId, { body, mentions })
      void mutate(meetingThreadsKey(options.meetingId))
      return true
    }
    catch (error) {
      console.debug('useMeetingChanges could not reply in a thread', { error, threadId })
      refused(error)
      return false
    }
  }

  async function setResolved(threadId: string, resolved: boolean) {
    try {
      const response = await trackMeetingChange(
        options.meetingId,
        setMeetingThreadResolved(options.meetingId, threadId, resolved),
      )
      made(response, resolved
        ? t('review.comments.resolved')
        : t('review.comments.reopened'))
    }
    catch (error) {
      console.debug('useMeetingChanges could not resolve or reopen a thread', { error, threadId, resolved })
      // Resolved or reopened elsewhere meanwhile: the page reads the thread as it stands.
      if (getApiErrorCode(error) === 'meeting_thread_unchanged') {
        options.reload()
        return
      }
      refused(error)
    }
  }

  // Removes thread `threadId`, comments and all; its toast's Undo puts it back whole.
  async function removeThread(threadId: string) {
    try {
      const response = await trackMeetingChange(options.meetingId, removeMeetingThread(options.meetingId, threadId))
      made(response, t('review.comments.removed'))
    }
    catch (error) {
      console.debug('useMeetingChanges could not remove a thread', { error, threadId })
      refused(error)
    }
  }

  // Tells the person a change to a range was refused: words changed meanwhile read the transcript
  // again, as an edit's do.
  function refusedOnWords(error: unknown) {
    if (getApiErrorCode(error) === 'meeting_words_changed') {
      options.reload()
      toast.warning(t('review.wordsChanged'))
      return
    }
    refused(error)
  }

  // Takes the words from `startWordId` to `endWordId` into project `projectId` as a new excerpt.
  // Resolves whether it did.
  async function assign(startWordId: string, endWordId: string, projectId: string, projectName: string) {
    try {
      const response = await trackMeetingChange(
        options.meetingId,
        assignMeetingExcerpt(options.meetingId, { startWordId, endWordId, projectIds: [ projectId ]}),
      )
      made(response, t('review.excerpts.assigned', { project: projectName }))
      return true
    }
    catch (error) {
      console.debug('useMeetingChanges could not assign words', { error, startWordId, endWordId })
      refusedOnWords(error)
      return false
    }
  }

  // Blacks out the words from `startWordId` to `endWordId`, with any black-out they touch.
  // Resolves whether it did.
  async function blackOut(startWordId: string, endWordId: string) {
    try {
      const response = await trackMeetingChange(
        options.meetingId,
        blackOutMeetingWords(options.meetingId, { startWordId, endWordId }),
      )
      made(response, t('review.blackOuts.made'))
      return true
    }
    catch (error) {
      console.debug('useMeetingChanges could not black words out', { error, startWordId, endWordId })
      refusedOnWords(error)
      return false
    }
  }

  // Removes excerpt or black-out `markId`; its toast's Undo puts it back.
  async function removeMark(markId: string, kind: 'excerpt' | 'black_out') {
    try {
      const response = await trackMeetingChange(options.meetingId, removeMeetingMark(options.meetingId, markId))
      made(response, kind === 'excerpt'
        ? t('review.excerpts.removed')
        : t('review.blackOuts.removed'))
    }
    catch (error) {
      console.debug('useMeetingChanges could not remove a mark', { error, markId })
      if (getApiErrorCode(error) === 'meeting_mark_removed') {
        options.reload()
        return
      }
      refused(error)
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

  return {
    edit,
    startThread,
    reply,
    setResolved,
    removeThread,
    assign,
    blackOut,
    removeMark,
    undo,
    undoLast,
  } as const
}

export type MeetingChanges = ReturnType<typeof useMeetingChanges>
