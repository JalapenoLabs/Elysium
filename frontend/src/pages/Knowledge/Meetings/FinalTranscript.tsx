// Copyright © 2026 Jalapeno Labs

import type { PointerEvent as ReactPointerEvent } from 'react'
import type { MeetingWord } from '../../../api/routes/meetingRoutes'
import type { MarkPill } from './TranscriptFloatingBar'
import type { ExcerptBar, Paragraph, PlacedMark, WordDecorations, WordRange } from './meetingTranscript'

// Core
import { useRef } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { TranscriptFloatingBar } from './TranscriptFloatingBar'
import { TranscriptParagraph } from './TranscriptParagraph'
import { WordRangeEditor } from './WordRangeEditor'

// Misc
import { textOfRange } from './meetingTranscript'

const NO_RANGE = { start: null, end: null } as const

type Props = {
  words: MeetingWord[]
  paragraphs: Paragraph[]
  decorations: WordDecorations
  laneCount: number
  barsByParagraph: Map<number, ExcerptBar[]>
  excerptLabels: Map<string, string>
  // What is picked: the words selected, the mark they came from, and the word a click chose.
  selection: WordRange | null
  // The word the selection ends at, where the bar floats.
  focus: number | null
  selectedMark: PlacedMark | null
  moment: number | null
  pills: MarkPill[]
  isEditing: boolean
  isExtending: boolean
  alternatives: string[]
  // A click on word `index`; with Shift, it extends what is picked to there.
  onPick: (index: number, shiftKey: boolean) => void
  onDrag: (anchor: number, focus: number) => void
  onSelectMark: (markId: string) => void
  onEdit: () => void
  onClear: () => void
  onApplyEdit: (text: string) => Promise<boolean>
  onCancelEdit: () => void
  onSelectFromHere: (() => void) | null
}

// The word a piece of the transcript stands for: a word itself, or the space after one.
function wordIndexOf(target: EventTarget | null) {
  if (!(target instanceof Element)) {
    return null
  }
  const piece = target.closest('[data-word-index], [data-gap-after]')
  const index = piece?.getAttribute('data-word-index') ?? piece?.getAttribute('data-gap-after')
  if (index === null || index === undefined) {
    return null
  }
  return Number(index)
}

// Clips a range to a paragraph, so only the paragraphs it reaches see it change.
function clip(range: WordRange | null, paragraph: Paragraph) {
  if (!range || range.end < paragraph.start || range.start > paragraph.end) {
    return NO_RANGE
  }
  return {
    start: Math.max(range.start, paragraph.start),
    end: Math.min(range.end, paragraph.end),
  }
}

type Drag = {
  pointerId: number
  anchor: number
  focus: number
}

// A meeting's final transcript, its marks drawn over its words (docs/meeting-review.md, The
// meeting's page). A click picks a moment, a drag or a click then Shift+click selects whole words,
// and the bar beside them acts on them. The browser's own text selection is off, since a range
// here is always whole words; Ctrl+C copies the words selected instead (`useReviewHotkeys`).
export function FinalTranscript(props: Props) {
  const { t } = useTranslation('meetings')
  const containerRef = useRef<HTMLDivElement>(null)
  const drag = useRef<Drag | null>(null)
  // A mouse press the drag already answered, as a selection or as a moment picked, ends in a click
  // too. Captured, that click lands on the transcript rather than a word, so it is let pass.
  const isAnswered = useRef(false)

  function startDrag(event: ReactPointerEvent<HTMLDivElement>) {
    isAnswered.current = false
    // A finger scrolls; it picks with a tap instead, and Shift+click extends with a click.
    if (event.pointerType !== 'mouse' || event.button !== 0 || event.shiftKey || props.isEditing) {
      return
    }
    const index = wordIndexOf(event.target)
    if (index === null) {
      return
    }
    event.preventDefault()
    drag.current = { pointerId: event.pointerId, anchor: index, focus: index }
    event.currentTarget.setPointerCapture(event.pointerId)
  }

  function moveDrag(event: ReactPointerEvent<HTMLDivElement>) {
    const current = drag.current
    if (!current || current.pointerId !== event.pointerId) {
      return
    }
    // Captured, the pointer's target is the transcript itself, so the word comes from the point.
    const index = wordIndexOf(document.elementFromPoint(event.clientX, event.clientY))
    if (index === null || index === current.focus) {
      return
    }
    current.focus = index
    props.onDrag(current.anchor, index)
  }

  function endDrag(event: ReactPointerEvent<HTMLDivElement>) {
    const current = drag.current
    if (current?.pointerId !== event.pointerId) {
      return
    }
    drag.current = null
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId)
    }
    // A press that never left its word is a click on it.
    if (event.type === 'pointerup' && current.focus === current.anchor) {
      props.onPick(current.anchor, false)
    }
    isAnswered.current = event.type === 'pointerup'
  }

  const anchorIndex = props.focus ?? props.moment
  const editor = props.isEditing && props.selection
    ? <WordRangeEditor
      key={`${props.selection.start}-${props.selection.end}`}
      text={textOfRange(props.words, props.selection)}
      alternatives={props.alternatives}
      onApply={props.onApplyEdit}
      onCancel={props.onCancelEdit}
    />
    : null

  return <div
    ref={containerRef}
    role='document'
    aria-label={t('review.transcriptLabel')}
    className='relative select-none'
    onPointerDown={startDrag}
    onPointerMove={moveDrag}
    onPointerUp={endDrag}
    onPointerCancel={endDrag}
    onClick={(event) => {
      if (isAnswered.current) {
        isAnswered.current = false
        return
      }
      const index = wordIndexOf(event.target)
      if (index === null || props.isEditing) {
        return
      }
      props.onPick(index, event.shiftKey)
    }}
  >
    {props.paragraphs.map((paragraph, index) => {
      const editing = props.isEditing
        ? clip(props.selection, paragraph)
        : NO_RANGE
      return <TranscriptParagraph
        key={props.words[paragraph.start].id}
        words={props.words}
        paragraph={paragraph}
        decorations={props.decorations}
        laneCount={props.laneCount}
        bars={props.barsByParagraph.get(index)}
        excerptLabels={props.excerptLabels}
        selection={props.isEditing
          ? NO_RANGE
          : clip(props.selection, paragraph)}
        selectedExcerpt={props.selectedMark?.kind === 'excerpt'
          ? clip(props.selectedMark, paragraph)
          : NO_RANGE}
        moment={props.moment !== null && paragraph.start <= props.moment && props.moment <= paragraph.end
          ? props.moment
          : null}
        editing={editing}
        editor={editing.start === props.selection?.start
          ? editor
          : null}
        onExcerptPress={props.onSelectMark}
      />
    })}

    {!props.isEditing && anchorIndex !== null && <TranscriptFloatingBar
      containerRef={containerRef}
      anchorIndex={anchorIndex}
      pills={props.pills}
      selectedMarkId={props.selectedMark?.mark.id ?? null}
      onSelectMark={props.onSelectMark}
      hasSelection={Boolean(props.selection)}
      onEdit={props.onEdit}
      onClear={props.onClear}
      onSelectFromHere={props.onSelectFromHere}
      isExtending={props.isExtending}
    />}
  </div>
}
