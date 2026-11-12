// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { MeetingAttachmentPolicy } from '../../../api/routes/meetingAttachmentRoutes'
import type {
  MeetingChangeResponse,
  MeetingHistoryEntry,
  MeetingTranscriptChange,
  TakeMeetingFrameRequest,
} from '../../../api/routes/meetingRoutes'

// Core
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { mutate } from 'swr'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { meetingUpserted } from '../../../store/meetingsSlice'

// Misc
import { getApiErrorCode, getApiErrorMessage } from '../../../api/errors'
import {
  assignMeetingExcerpt,
  blackOutMeetingWords,
  detachMeetingAttachment,
  editMeetingWords,
  meetingAttachmentsKey,
  meetingHistoryKey,
  meetingThreadsKey,
  pinMeetingAttachment,
  removeMeetingMark,
  removeMeetingThread,
  replyInMeetingThread,
  setCaptureOffset,
  setMeetingThreadResolved,
  splitCapture,
  startMeetingThread,
  takeMeetingFrame,
  undoMeetingChange,
} from '../../../api/routes/meetingRoutes'
import { createMeetingUpload, finishAttachmentUpload } from '../../../api/routes/meetingUploadRoutes'
import { timelineKeyPrefix } from '../../../api/routes/timelineRoutes'
import { formatBytes } from '../../../format/numbers'
import { trackMeetingChange } from '../../../realtime/meetingChangeEchoes'
import { toast } from '../../../toast/toast'
import { sendMeetingFiles } from '../../MeetingUploads/sendMeetingFiles'
import { uploadProgress } from '../../MeetingUploads/meetingUpload'
import { refusalOf } from './meetingAttachments'

// What a split refused says, by the API's code: the capture went back already, or a capture the
// merge brought was moved by hand since, which splitting would overwrite.
const splitRefusalKeys = {
  capture_not_merged: 'review.captures.notMerged',
  meeting_changed: 'review.captures.movedSince',
} as const satisfies Record<string, ParseKeys<'meetings'>>

// A file on its way to the meeting: its name, and the share of it that has arrived, 0 to 1.
export type AttachingFile = {
  key: string
  name: string
  progress: number
}

// What can be removed as a mark from a meeting's page, each with what its toast says.
export type RemovableMarkKind = 'excerpt' | 'black_out' | 'pin'

const removedKeys = {
  excerpt: 'review.excerpts.removed',
  black_out: 'review.blackOuts.removed',
  pin: 'review.files.unpinned',
} as const satisfies Record<RemovableMarkKind, ParseKeys<'meetings'>>

// Why a file is not sent, in the person's words.
const attachRefusalKeys = {
  empty: 'review.files.refusedEmpty',
  tooLarge: 'review.files.refusedTooLarge',
  nowhere: 'review.files.refusedNowhere',
} as const satisfies Record<'empty' | 'tooLarge' | 'nowhere', ParseKeys<'meetings'>>

type Options = {
  meetingId: string
  // Whether the meeting takes no change, as one merged into another does. It can become so while
  // the page holds changes it made, so Ctrl+Z, and Undo on a toast still showing, refuse then.
  isReadOnly: boolean
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
  const [ attaching, setAttaching ] = useState<AttachingFile[]>([])
  // Read as Undo is pressed, not as it was offered: a toast's Undo keeps the function of the
  // render that showed it, which saw the meeting before it was merged away.
  const isReadOnly = useRef(options.isReadOnly)
  useEffect(() => {
    isReadOnly.current = options.isReadOnly
  }, [ options.isReadOnly ])

  // Its own change applies from what the route answered; only the history and the threads' comments,
  // both small, are read again, since a change to a thread, or an undo of one, changes which threads
  // stand. The change's own announcement is then no news to this page (`meetingChangeEchoes`).
  function settle(response: MeetingChangeResponse) {
    options.applyChange(response.change)
    void mutate(meetingHistoryKey(options.meetingId))
    void mutate(meetingThreadsKey(options.meetingId))
    // A file or frame attached, taken off, or put back, by itself or by an undo.
    void mutate(meetingAttachmentsKey(options.meetingId))
    // An excerpt made, removed, or put back is a record on its projects' timelines.
    if (typeof response.entry.data.recordId === 'string') {
      void mutate((key) => typeof key === 'string' && key.startsWith(timelineKeyPrefix))
    }
  }

  // Joins a change made here to what Ctrl+Z undoes, and says so with Undo.
  function remember(entry: MeetingHistoryEntry, confirmation: string) {
    madeHere.current.push(entry.id)
    toast.success(confirmation, {
      action: {
        label: t('review.undo'),
        onPress: () => void undo(entry.id),
      },
    })
  }

  // Applies a change made here, and says so with Undo.
  function made(response: MeetingChangeResponse, confirmation: string) {
    settle(response)
    remember(response.entry, confirmation)
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
    if (isReadOnly.current) {
      console.debug('useMeetingChanges left a change as it is, since the meeting takes no change now', { entryId })
      toast(t('page.mergedReadOnly'))
      return
    }
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

  // Removes excerpt, black-out, or pin `markId`; its toast's Undo puts it back.
  async function removeMark(markId: string, kind: RemovableMarkKind) {
    try {
      const response = await trackMeetingChange(options.meetingId, removeMeetingMark(options.meetingId, markId))
      made(response, t(removedKeys[kind]))
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

  // A change that moves a capture lines the whole transcript up again, which no change of words
  // describes, so its announcement names no entry and every page holding the meeting reads it
  // again, this one included (`announce_lined_up` in api/src/routes/v1/meetings/mod.rs). It applies
  // nothing itself, and still joins what Ctrl+Z undoes.

  // Sets where capture `captureId` starts, `offsetMs` into the meeting, by hand. Resolves whether
  // it did; one refused keeps the editor open with what was typed.
  async function moveCapture(captureId: string, offsetMs: number) {
    try {
      const response = await setCaptureOffset(options.meetingId, captureId, offsetMs)
      remember(response.entry, t('review.captures.moved'))
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
      remember(response.entry, t('review.captures.splitDone'))
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

  // Pins `still`, the player's picture at a moment of a capture, as a frame with what it shows,
  // over the words selected or, with none, the word being spoken then. Resolves whether it did.
  async function takeFrame(request: TakeMeetingFrameRequest, still: Blob) {
    try {
      const response = await trackMeetingChange(
        options.meetingId,
        takeMeetingFrame(options.meetingId, request, still),
      )
      made(response, t('review.files.framePinned'))
      return true
    }
    catch (error) {
      console.debug('useMeetingChanges could not pin a frame', { error, captureId: request.captureId })
      refusedOnWords(error)
      return false
    }
  }

  // Pins file or frame `attachmentId` over the words from `startWordId` to `endWordId`, such as an
  // excerpt's, which then carries it to its projects.
  async function pin(attachmentId: string, startWordId: string, endWordId: string) {
    try {
      const response = await trackMeetingChange(
        options.meetingId,
        pinMeetingAttachment(options.meetingId, { attachmentId, startWordId, endWordId }),
      )
      made(response, t('review.files.pinned'))
    }
    catch (error) {
      console.debug('useMeetingChanges could not pin an attachment', { error, attachmentId })
      refusedOnWords(error)
    }
  }

  // Takes file or frame `attachmentId` off the meeting with its pins; its Undo puts both back.
  async function detach(attachmentId: string, name: string) {
    try {
      const response = await trackMeetingChange(
        options.meetingId,
        detachMeetingAttachment(options.meetingId, attachmentId),
      )
      made(response, t('review.files.detached', { name }))
    }
    catch (error) {
      console.debug('useMeetingChanges could not take an attachment off', { error, attachmentId })
      if (getApiErrorCode(error) === 'meeting_attachment_removed') {
        void mutate(meetingAttachmentsKey(options.meetingId))
        return
      }
      refused(error)
    }
  }

  // Attaches `file` to the meeting, in parts as a recording is sent, refused first as the API would
  // refuse it. Its toast's Undo takes it off again.
  async function attachFile(file: File, policy: MeetingAttachmentPolicy) {
    const refusal = refusalOf(file, policy)
    if (refusal) {
      console.debug('useMeetingChanges refused a file before sending it', { name: file.name, refusal })
      toast.danger(t('review.files.refused', { name: file.name }), {
        description: t(attachRefusalKeys[refusal], { size: formatBytes(policy.maxBytes) }),
      })
      return
    }
    const key = `${file.name}-${file.size}-${file.lastModified}-${Date.now()}`
    setAttaching((current) => [ ...current, { key, name: file.name, progress: 0 }])
    const setProgress = (progress: number) => setAttaching((current) => current.map((attachingFile) => (
      attachingFile.key === key
        ? { ...attachingFile, progress }
        : attachingFile
    )))
    try {
      const created = await createMeetingUpload({
        meetingId: options.meetingId,
        attachment: {
          name: file.name,
          sizeBytes: file.size,
          modifiedAt: new Date(file.lastModified).toISOString(),
        },
      })
      const sent = await sendMeetingFiles({
        upload: created.upload,
        files: { attachment: file },
        signal: new AbortController().signal,
        onProgress: (upload) => {
          const { received, total } = uploadProgress(upload)
          setProgress(total
            ? received / total
            : 1)
        },
      })
      const finished = await finishAttachmentUpload(sent.id)
      void mutate(meetingAttachmentsKey(options.meetingId))
      void mutate(meetingHistoryKey(options.meetingId))
      if (finished.entry) {
        remember(finished.entry, t('review.files.attached', { name: finished.attachment.fileName }))
      }
    }
    catch (error) {
      console.debug('useMeetingChanges could not attach a file', { error, name: file.name })
      toast.danger(t('review.files.refused', { name: file.name }), {
        description: getApiErrorMessage(error) ?? t('common:errors.unexpected'),
      })
    }
    finally {
      setAttaching((current) => current.filter((attachingFile) => attachingFile.key !== key))
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
    moveCapture,
    splitOut,
    takeFrame,
    pin,
    detach,
    attachFile,
    attaching,
    undo,
    undoLast,
  } as const
}

export type MeetingChanges = ReturnType<typeof useMeetingChanges>
