// Copyright © 2026 Jalapeno Labs

import type { Meeting, MeetingTranscriptChange, MeetingWords } from '../../../api/routes/meetingRoutes'
import type { MarkPill } from './TranscriptFloatingBar'
import type { DrawnMarkKind, WordRange } from './meetingTranscript'

// Core
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useSearchParams } from 'react-router'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectProjectEntities } from '../../../store/projectsSlice'

// User interface
import { Button, Drawer, useOverlayState } from '@heroui/react'
import { LuSlidersHorizontal } from 'react-icons/lu'
import { EmptyNotice } from '../../../components/EmptyNotice'
import { CaptureTranscript } from './CaptureTranscript'
import { FinalTranscript } from './FinalTranscript'
import { MeetingLane } from './MeetingLane'
import { FINAL_VIEW } from './MeetingOptions'
import { MeetingPlayer } from './MeetingPlayer'

// Misc
import { useProjectsLoader, useUsersLoader } from '../../../hooks/useServerData'
import { PLAYER_JUMP_MS } from './meetingPlayer'
import { markKindLabelKeys } from './meetingReviewPresentation'
import {
  alternativesFor,
  decorateWords,
  DRAWN_MARK_KINDS,
  excerptBarsByParagraph,
  excerptLanes,
  marksUnder,
  paragraphsOf,
  placeMarks,
  rangeBetween,
  summarizeRange,
  textOfRange,
} from './meetingTranscript'
import { useMeetingChanges } from './useMeetingChanges'
import { isResolvedThread, threadsOf } from './meetingThreads'
import { useMeetingHistory } from './useMeetingHistory'
import { useMeetingPlayer } from './useMeetingPlayer'
import { useMeetingThreads } from './useMeetingThreads'
import { useReviewHotkeys } from './useReviewHotkeys'

type Props = {
  meeting: Meeting
  transcript: MeetingWords
  applyChange: (change: MeetingTranscriptChange) => void
  reload: () => void
}

// What is picked on the transcript, by word id, so it stays on the same words when the transcript
// is read again around it: the words selected, from the word they started at to the one they
// reach, the mark they came from when a click chose one, and the word a click picked as its
// moment. A word that no longer stands picks nothing.
type Review = {
  anchorId: string | null
  focusId: string | null
  markId: string | null
  momentId: string | null
  isEditing: boolean
  // A new comment thread is being written on the words selected, in the lane.
  isCommenting: boolean
  // On a touch screen, which has no Shift, the next tap ends a selection started at the moment.
  isExtending: boolean
}

const NOTHING_PICKED: Review = {
  anchorId: null,
  focusId: null,
  markId: null,
  momentId: null,
  isEditing: false,
  isCommenting: false,
  isExtending: false,
}

// Where the lane stands beside the transcript rather than in a drawer: Tailwind's `lg`, which the
// layout below switches at.
const LANE_BESIDE_QUERY = '(min-width: 64rem)'

// Brings word `index` of the transcript into view, for a thread opened from the lane or a link.
function revealWord(index: number) {
  document
    .querySelector(`[data-word-index="${index}"]`)
    ?.scrollIntoView({ block: 'center', behavior: 'smooth' })
}

// A meeting's review (docs/meeting-review.md, The meeting's page): the transcript, three quarters
// wide, and its lane beside it, which on a phone is a drawer. Along the top go the meeting's player
// and, as they are built, its tasks (#243); the moment a click picks is where the player seeks, and
// the word it is speaking is underlined as it plays. Every change applies at once with Undo in its
// toast, and Ctrl+Z undoes the page's last.
export function MeetingWorkspace(props: Props) {
  const { t } = useTranslation('meetings')
  const { meeting, transcript } = props
  const words = transcript.words
  // A meeting merged into another changes there, where its recordings went (docs/meetings.md, The
  // same meeting twice): its page reads as it stood, its words selected and copied, with nothing to
  // edit, comment on, or undo. Its actions say why (`MeetingActions`).
  const isReadOnly = Boolean(meeting.mergedInto)
  useUsersLoader()
  useProjectsLoader()
  const projects = useAppSelector(selectProjectEntities)
  const drawer = useOverlayState()
  const history = useMeetingHistory(meeting.id)
  const threadComments = useMeetingThreads(meeting.id)
  // A link to a thread, such as from the inbox item a mention raised, opens it once its words load.
  const [ searchParams ] = useSearchParams()
  const linkedThreadId = searchParams.get('thread')
  const hasOpenedLink = useRef(false)
  const changes = useMeetingChanges({
    meetingId: meeting.id,
    applyChange: props.applyChange,
    reload: props.reload,
  })
  const player = useMeetingPlayer({ meetingId: meeting.id, captures: transcript.captures, words })
  // Read once: a phone does not become a mouse while its page is open.
  const [ isTouch ] = useState(() => window.matchMedia('(pointer: coarse)').matches)

  const [ view, setView ] = useState(FINAL_VIEW)
  const [ shown, setShown ] = useState<ReadonlySet<DrawnMarkKind>>(() => new Set(DRAWN_MARK_KINDS))
  const [ review, setReview ] = useState(NOTHING_PICKED)

  // Everything drawn is derived once per transcript and per switch of the marks shown, so a
  // selection redraws only the paragraphs it reaches.
  const indexById = useMemo(() => new Map(words.map((word, index) => [ word.id, index ])), [ words ])
  const placed = useMemo(() => placeMarks(words, transcript.marks), [ words, transcript.marks ])
  // A resolved thread is done with: it stays in the lane, and its words draw no outline.
  const drawn = useMemo(
    () => placed.filter((placedMark) => placedMark.kind !== 'comment' || !isResolvedThread(placedMark.mark)),
    [ placed ],
  )
  const threads = useMemo(
    () => threadsOf(placed, threadComments.comments ?? []),
    [ placed, threadComments.comments ],
  )
  const paragraphs = useMemo(() => paragraphsOf(words), [ words ])
  const decorations = useMemo(() => decorateWords(words.length, drawn, shown), [ words.length, drawn, shown ])
  // Read only, every change stays as it is: no row of the history, nor an edit's mark, offers Undo.
  const historyRows = useMemo(
    () => isReadOnly
      ? history.rows.map((row) => ({ ...row, canUndo: false }))
      : history.rows,
    [ history.rows, isReadOnly ],
  )
  const excerpts = useMemo(() => {
    const lanes = shown.has('excerpt')
      ? excerptLanes(placed)
      : new Map<string, number>()
    // Each excerpt names its projects over its bar (#241 writes `projectIds`).
    const labels = new Map<string, string>()
    for (const placedMark of placed) {
      if (placedMark.kind !== 'excerpt' || !Array.isArray(placedMark.mark.data.projectIds)) {
        continue
      }
      const names: string[] = []
      for (const projectId of placedMark.mark.data.projectIds) {
        const name = typeof projectId === 'string'
          ? projects[projectId]?.name
          : undefined
        if (name) {
          names.push(name)
        }
      }
      if (names.length) {
        labels.set(placedMark.mark.id, names.join(', '))
      }
    }
    return {
      laneCount: lanes.size
        ? Math.max(...lanes.values()) + 1
        : 0,
      bars: excerptBarsByParagraph(paragraphs, placed, lanes),
      labels,
    }
  }, [ placed, paragraphs, projects, shown ])

  const anchor = review.anchorId === null
    ? undefined
    : indexById.get(review.anchorId)
  const focus = review.focusId === null
    ? undefined
    : indexById.get(review.focusId)
  const moment = review.momentId === null
    ? null
    : indexById.get(review.momentId) ?? null
  const selection: WordRange | null = anchor !== undefined && focus !== undefined
    ? rangeBetween(anchor, focus)
    : null
  const selectedMark = selection
    ? placed.find((placedMark) => placedMark.mark.id === review.markId) ?? null
    : null
  const under = moment === null
    ? []
    : marksUnder(drawn, moment, shown)
  const pills: MarkPill[] = under.map((placedMark) => ({
    markId: placedMark.mark.id,
    label: t(markKindLabelKeys[placedMark.kind]),
  }))

  const pickRange = useCallback((start: number, end: number, extra: Partial<Review> = {}) => {
    setReview({
      ...NOTHING_PICKED,
      anchorId: words[start].id,
      focusId: words[end].id,
      ...extra,
    })
  }, [ words ])

  const selectMarkById = useCallback((markId: string) => {
    const placedMark = placed.find((candidate) => candidate.mark.id === markId)
    if (!placedMark) {
      console.debug('MeetingWorkspace was asked to select a mark it does not hold', { markId })
      return
    }
    // A mark chosen from its pills keeps the moment its word was clicked at.
    setReview((current) => {
      const momentIndex = current.momentId === null
        ? undefined
        : indexById.get(current.momentId)
      const keepsMoment = momentIndex !== undefined
        && placedMark.start <= momentIndex
        && momentIndex <= placedMark.end
      return {
        ...NOTHING_PICKED,
        anchorId: words[placedMark.start].id,
        focusId: words[placedMark.end].id,
        markId,
        momentId: keepsMoment
          ? current.momentId
          : null,
      }
    })
  }, [ placed, indexById, words ])

  // A click picks a moment, which the player seeks, and selects the shortest mark under it
  // (docs/meeting-review.md, A click picks a moment). With Shift, or a tap after Select from here,
  // it extends what is picked to the word instead.
  function onPick(index: number, shiftKey: boolean) {
    // From the word clicked, even when that click selected the mark around it; else from where a
    // drag started.
    const from = moment ?? anchor
    if ((shiftKey || review.isExtending) && from !== undefined && from !== null) {
      pickRange(from, index)
      return
    }
    if (player.capture) {
      player.seek(words[index].startMs)
    }
    const [ shortest ] = marksUnder(drawn, index, shown)
    if (shortest) {
      pickRange(shortest.start, shortest.end, { markId: shortest.mark.id, momentId: words[index].id })
      return
    }
    setReview({ ...NOTHING_PICKED, momentId: words[index].id })
  }

  const clear = () => setReview(NOTHING_PICKED)

  // A thread chosen from the lane, or from a link, selects its range and brings it into view.
  const openThread = useCallback((threadId: string) => {
    const thread = placed.find((placedMark) => placedMark.mark.id === threadId)
    if (!thread) {
      console.debug('MeetingWorkspace was asked to open a thread it does not hold', { threadId })
      return
    }
    selectMarkById(threadId)
    revealWord(thread.start)
  }, [ placed, selectMarkById ])

  // The thread a link names opens where it can be read: beside the transcript, or in the drawer
  // where the lane is one.
  const { open: openDrawer } = drawer
  useEffect(() => {
    if (!linkedThreadId || hasOpenedLink.current || !words.length) {
      return
    }
    hasOpenedLink.current = true
    openThread(linkedThreadId)
    if (!window.matchMedia(LANE_BESIDE_QUERY).matches) {
      openDrawer()
    }
  }, [ linkedThreadId, words.length, openThread, openDrawer ])

  // Opens the words selected in place to edit them. Answers whether it did.
  function startEdit() {
    if (isReadOnly || view !== FINAL_VIEW || !selection || review.isEditing) {
      return false
    }
    setReview({ ...review, isEditing: true, isCommenting: false })
    return true
  }

  // Starts writing a thread on the words selected; the lane holds it, so a phone opens its drawer.
  function startComment() {
    if (isReadOnly || view !== FINAL_VIEW || !selection || review.isEditing) {
      return false
    }
    setReview({ ...review, isCommenting: true })
    if (!window.matchMedia(LANE_BESIDE_QUERY).matches) {
      drawer.open()
    }
    return true
  }

  async function postThread(body: string, mentions: string[]) {
    if (!selection) {
      console.debug('MeetingWorkspace was asked to post a thread with nothing selected')
      return false
    }
    const threadId = await changes.startThread(words[selection.start].id, words[selection.end].id, body, mentions)
    if (threadId === null) {
      return false
    }
    // The new thread opens where it was written.
    setReview({
      ...NOTHING_PICKED,
      anchorId: words[selection.start].id,
      focusId: words[selection.end].id,
      markId: threadId,
    })
    return true
  }

  // Where Play from here starts: the words selected, else the word clicked.
  const playStart = selection?.start ?? moment
  const playFromHere = player.capture && playStart !== null
    ? () => player.playFrom(words[playStart].startMs)
    : null

  async function applyEdit(text: string) {
    if (!selection) {
      return false
    }
    const applied = await changes.edit(words[selection.start].id, words[selection.end].id, text)
    if (applied) {
      clear()
    }
    return applied
  }

  useReviewHotkeys({
    onEscape: () => {
      if (!selection && moment === null) {
        return false
      }
      clear()
      return true
    },
    onEdit: startEdit,
    onComment: startComment,
    onCycleMark: (step) => {
      if (under.length < 2) {
        return false
      }
      const current = under.findIndex((placedMark) => placedMark.mark.id === review.markId)
      const next = under[(current + step + under.length) % under.length]
      selectMarkById(next.mark.id)
      return true
    },
    // Space pauses what plays; otherwise it plays the words selected, as Play from here does, or
    // plays on from where the player is.
    onPlayPause: (isRepeat) => {
      if (!player.capture) {
        return false
      }
      if (isRepeat) {
        return true
      }
      if (player.isPlaying) {
        player.toggle()
      }
      else if (selection) {
        player.playFrom(words[selection.start].startMs)
      }
      else {
        player.play()
      }
      return true
    },
    onJump: (step) => {
      if (!player.capture) {
        return false
      }
      player.jump(step * PLAYER_JUMP_MS)
      return true
    },
    onUndo: () => void changes.undoLast(),
    selectedText: () => selection && !review.isEditing
      ? textOfRange(words, selection)
      : null,
  })

  const lane = (isInDrawer: boolean) => <MeetingLane
    meeting={meeting}
    captures={transcript.captures}
    view={view}
    onViewChange={(next) => {
      setView(next)
      clear()
    }}
    shown={shown}
    onShownChange={(kind, isShown) => {
      const next = new Set(shown)
      if (isShown) {
        next.add(kind)
      }
      else {
        next.delete(kind)
      }
      setShown(next)
      // A mark of a kind hidden would stay selected out of sight.
      if (!isShown && selectedMark?.kind === kind) {
        clear()
      }
    }}
    span={selection
      ? summarizeRange(words, selection)
      : null}
    momentMs={moment === null
      ? null
      : words[moment].startMs}
    onMoveCapture={changes.moveCapture}
    onSplitCapture={changes.splitOut}
    words={words}
    selectedMark={selectedMark}
    history={{
      rows: historyRows,
      isLoading: !history.entries && !history.hasFailed,
      hasFailed: history.hasFailed,
    }}
    onUndo={changes.undo}
    threads={{
      open: threads.open,
      resolved: threads.resolved,
      isLoading: !threadComments.comments && !threadComments.hasFailed,
      hasFailed: threadComments.hasFailed,
    }}
    threadActions={isReadOnly
      ? null
      : {
        onReply: changes.reply,
        onSetResolved: changes.setResolved,
        onRemove: changes.removeThread,
      }}
    onSelectThread={openThread}
    composing={review.isCommenting && selection
      ? {
        range: selection,
        onPost: postThread,
        onCancel: () => setReview({ ...review, isCommenting: false }),
      }
      : null}
    isInDrawer={isInDrawer}
  />

  let body = <CaptureTranscript meetingId={meeting.id} captureId={view} />
  if (view === FINAL_VIEW && !words.length) {
    body = <EmptyNotice>{t('review.notBuilt')}</EmptyNotice>
  }
  else if (view === FINAL_VIEW) {
    body = <FinalTranscript
      words={words}
      paragraphs={paragraphs}
      decorations={decorations}
      laneCount={excerpts.laneCount}
      barsByParagraph={excerpts.bars}
      excerptLabels={excerpts.labels}
      selection={selection}
      focus={focus ?? null}
      selectedMark={selectedMark}
      moment={moment}
      spoken={player.spokenIndex}
      pills={pills}
      isEditing={review.isEditing && Boolean(selection)}
      isExtending={review.isExtending}
      alternatives={selection
        ? alternativesFor(placed, selection)
        : []}
      onPick={onPick}
      onDrag={(start, end) => pickRange(start, end)}
      onSelectMark={selectMarkById}
      onEdit={isReadOnly
        ? null
        : startEdit}
      onComment={isReadOnly
        ? null
        : startComment}
      onClear={clear}
      onApplyEdit={applyEdit}
      onCancelEdit={() => setReview({ ...review, isEditing: false })}
      onSelectFromHere={isTouch && moment !== null
        ? () => pickRange(moment, moment, { isExtending: true })
        : null}
      onPlayFromHere={playFromHere}
    />
  }

  return <>
    <MeetingPlayer player={player} />

    <div className='lg:grid lg:grid-cols-[minmax(0,3fr)_minmax(15rem,1fr)] lg:gap-8'>
      <div className='min-w-0'>
        <div className='relaxed lg:hidden'>
          <Button size='sm' variant='outline' onPress={drawer.open}>
            <LuSlidersHorizontal className='size-4' aria-hidden />
            <span>{t('review.options.open')}</span>
          </Button>
        </div>
        {body}
      </div>
      <aside className='hidden lg:block'>{lane(false)}</aside>
    </div>

    {/* Controlled, so the root trigger wrapper is skipped, as in KnowledgeFrame. */}
    <Drawer.Backdrop isOpen={drawer.isOpen} onOpenChange={drawer.setOpen}>
      <Drawer.Content placement='right'>
        <Drawer.Dialog className='flex flex-col gap-4'>
          <Drawer.Header>
            <Drawer.Heading className='text-lg'>{t('review.options.title')}</Drawer.Heading>
          </Drawer.Header>
          <Drawer.Body className='min-h-0 flex-1 overflow-y-auto'>{lane(true)}</Drawer.Body>
          <Drawer.Footer className='flex-col items-stretch'>
            <Button fullWidth variant='secondary' onPress={drawer.close}>
              <span>{t('review.options.close')}</span>
            </Button>
          </Drawer.Footer>
        </Drawer.Dialog>
      </Drawer.Content>
    </Drawer.Backdrop>
  </>
}
