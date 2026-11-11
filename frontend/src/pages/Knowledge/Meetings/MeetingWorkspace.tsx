// Copyright © 2026 Jalapeno Labs

import type { Meeting, MeetingTranscriptChange, MeetingWords } from '../../../api/routes/meetingRoutes'
import type { MarkPill } from './TranscriptFloatingBar'
import type { DrawnMarkKind, WordRange } from './meetingTranscript'

// Core
import { useCallback, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

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

// Misc
import { useProjectsLoader, useUsersLoader } from '../../../hooks/useServerData'
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
import { useMeetingHistory } from './useMeetingHistory'
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
  // On a touch screen, which has no Shift, the next tap ends a selection started at the moment.
  isExtending: boolean
}

const NOTHING_PICKED: Review = {
  anchorId: null,
  focusId: null,
  markId: null,
  momentId: null,
  isEditing: false,
  isExtending: false,
}

// A meeting's review (docs/meeting-review.md, The meeting's page): the transcript, three quarters
// wide, and its lane beside it, which on a phone is a drawer. Along the top go the meeting's tasks
// (#243) and its player (#236) as they are built; the moment a click picks is what the player will
// seek. Every change applies at once with Undo in its toast, and Ctrl+Z undoes the page's last.
export function MeetingWorkspace(props: Props) {
  const { t } = useTranslation('meetings')
  const { meeting, transcript } = props
  const words = transcript.words
  useUsersLoader()
  useProjectsLoader()
  const projects = useAppSelector(selectProjectEntities)
  const drawer = useOverlayState()
  const history = useMeetingHistory(meeting.id)
  const changes = useMeetingChanges({
    meetingId: meeting.id,
    applyChange: props.applyChange,
    reload: props.reload,
  })
  // Read once: a phone does not become a mouse while its page is open.
  const [ isTouch ] = useState(() => window.matchMedia('(pointer: coarse)').matches)

  const [ view, setView ] = useState(FINAL_VIEW)
  const [ shown, setShown ] = useState<ReadonlySet<DrawnMarkKind>>(() => new Set(DRAWN_MARK_KINDS))
  const [ review, setReview ] = useState(NOTHING_PICKED)

  // Everything drawn is derived once per transcript and per switch of the marks shown, so a
  // selection redraws only the paragraphs it reaches.
  const indexById = useMemo(() => new Map(words.map((word, index) => [ word.id, index ])), [ words ])
  const placed = useMemo(() => placeMarks(words, transcript.marks), [ words, transcript.marks ])
  const paragraphs = useMemo(() => paragraphsOf(words), [ words ])
  const decorations = useMemo(() => decorateWords(words.length, placed, shown), [ words.length, placed, shown ])
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
    : marksUnder(placed, moment, shown)
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

  // A click picks a moment, and selects the shortest mark under it (docs/meeting-review.md, A
  // click picks a moment). With Shift, or a tap after Select from here, it extends what is picked
  // to the word instead.
  function onPick(index: number, shiftKey: boolean) {
    // From the word clicked, even when that click selected the mark around it; else from where a
    // drag started.
    const from = moment ?? anchor
    if ((shiftKey || review.isExtending) && from !== undefined && from !== null) {
      pickRange(from, index)
      return
    }
    const [ shortest ] = marksUnder(placed, index, shown)
    if (shortest) {
      pickRange(shortest.start, shortest.end, { markId: shortest.mark.id, momentId: words[index].id })
      return
    }
    setReview({ ...NOTHING_PICKED, momentId: words[index].id })
  }

  const clear = () => setReview(NOTHING_PICKED)

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
    onEdit: () => {
      if (view !== FINAL_VIEW || !selection || review.isEditing) {
        return false
      }
      setReview({ ...review, isEditing: true })
      return true
    },
    onCycleMark: (step) => {
      if (under.length < 2) {
        return false
      }
      const current = under.findIndex((placedMark) => placedMark.mark.id === review.markId)
      const next = under[(current + step + under.length) % under.length]
      selectMarkById(next.mark.id)
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
      rows: history.rows,
      isLoading: !history.entries && !history.hasFailed,
      hasFailed: history.hasFailed,
    }}
    onUndo={changes.undo}
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
      pills={pills}
      isEditing={review.isEditing && Boolean(selection)}
      isExtending={review.isExtending}
      alternatives={selection
        ? alternativesFor(placed, selection)
        : []}
      onPick={onPick}
      onDrag={(start, end) => pickRange(start, end)}
      onSelectMark={selectMarkById}
      onEdit={() => setReview({ ...review, isEditing: true })}
      onClear={clear}
      onApplyEdit={applyEdit}
      onCancelEdit={() => setReview({ ...review, isEditing: false })}
      onSelectFromHere={isTouch && moment !== null
        ? () => pickRange(moment, moment, { isExtending: true })
        : null}
    />
  }

  return <>
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
