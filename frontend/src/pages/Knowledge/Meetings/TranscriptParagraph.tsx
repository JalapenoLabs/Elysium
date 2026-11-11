// Copyright © 2026 Jalapeno Labs

import type { CSSProperties, ReactNode } from 'react'
import type { MeetingWord } from '../../../api/routes/meetingRoutes'
import type { ExcerptBar, Paragraph, WordDecorations } from './meetingTranscript'

// Core
import { memo } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Tooltip } from '@heroui/react'
import { Button as AriaButton } from 'react-aria-components'

// Misc
import { formatMediaPosition } from '../../../format/durations'
import { markKindBits } from './meetingTranscript'

// Each drawn kind's attribute on a piece (`src/theme/meetingTranscript.css`). Confidence is the
// background and excerpts are gutter bars, so neither marks a piece by kind.
const pieceAttributeByKind = [
  [ markKindBits.edit, 'edit' ],
  [ markKindBits.black_out, 'black-out' ],
  [ markKindBits.comment, 'comment' ],
] as const

// The width of one excerpt lane in the gutter, in rem.
const EXCERPT_LANE_REM = 0.5

type Range = {
  start: number | null
  end: number | null
}

type Props = {
  words: MeetingWord[]
  paragraph: Paragraph
  decorations: WordDecorations
  // Every paragraph's gutter is as wide as the transcript's widest overlap, so the text lines up.
  laneCount: number
  bars: ExcerptBar[] | undefined
  // What each excerpt is about, shown over its bar.
  excerptLabels: Map<string, string>
  // Each of these is clipped to this paragraph by the transcript, and both ends are null when it
  // reaches none of it, so a selection redraws only the paragraphs it touches.
  selection: Range
  selectedExcerpt: Range
  moment: number | null
  editing: Range
  // The editor, given only to the paragraph where the edited range starts.
  editor: ReactNode
  onExcerptPress: (markId: string) => void
}

function within(range: Range, index: number) {
  return range.start !== null && range.end !== null && range.start <= index && index <= range.end
}

// The transcript clips ranges anew on every render, so they compare by their ends: a paragraph a
// change does not reach keeps what it drew.
function isSameParagraph(previous: Props, next: Props) {
  const isSameRange = (key: 'selection' | 'selectedExcerpt' | 'editing') => previous[key].start === next[key].start
    && previous[key].end === next[key].end
  return previous.words === next.words
    && previous.paragraph === next.paragraph
    && previous.decorations === next.decorations
    && previous.laneCount === next.laneCount
    && previous.bars === next.bars
    && previous.excerptLabels === next.excerptLabels
    && previous.moment === next.moment
    && previous.editor === next.editor
    && previous.onExcerptPress === next.onExcerptPress
    && isSameRange('selection')
    && isSameRange('selectedExcerpt')
    && isSameRange('editing')
}

// What one piece carries for the stylesheet: a word, or the space after word `index` when `isGap`.
function pieceAttributes(props: Props, index: number, isGap: boolean) {
  const { decorations } = props
  const attributes: Record<string, string> = {}
  const covered = isGap
    ? decorations.gapCovered[index]
    : decorations.covered[index]
  for (const [ bit, name ] of pieceAttributeByKind) {
    if (!(covered & bit)) {
      continue
    }
    attributes[`data-${name}`] = ''
    if (!isGap && decorations.starts[index] & bit) {
      attributes[`data-${name}-start`] = ''
    }
    if (!isGap && decorations.ends[index] & bit) {
      attributes[`data-${name}-end`] = ''
    }
  }

  // A space is inside a range when the words on both sides are.
  const isSelected = isGap
    ? within(props.selection, index) && within(props.selection, index + 1)
    : within(props.selection, index)
  if (isSelected) {
    attributes['data-selected'] = ''
  }
  const inExcerpt = isGap
    ? within(props.selectedExcerpt, index) && within(props.selectedExcerpt, index + 1)
    : within(props.selectedExcerpt, index)
  if (inExcerpt) {
    attributes['data-excerpt-selected'] = ''
    if (!isGap && index === props.selectedExcerpt.start) {
      attributes['data-excerpt-selected-start'] = ''
    }
    if (!isGap && index === props.selectedExcerpt.end) {
      attributes['data-excerpt-selected-end'] = ''
    }
  }
  if (!isGap && props.moment === index) {
    attributes['data-moment'] = ''
  }

  const confidence = isGap
    ? decorations.gapConfidence[index]
    : decorations.confidence[index]
  let style: CSSProperties | undefined
  if (confidence !== null) {
    attributes['data-confidence'] = String(confidence)
    // Red at 0, yellow at 50, green at 100.
    style = { '--confidence-hue': confidence * 1.2 } as CSSProperties
  }
  return { attributes, style } as const
}

// One paragraph of a meeting's final transcript: its speaker and time when it opens their turn,
// its excerpts' bars in the gutter, and its words, each mark drawn over them. Memoized, so a
// selection, an edit, or a moment redraws only the paragraphs it reaches.
export const TranscriptParagraph = memo(function TranscriptParagraph(props: Props) {
  const { t } = useTranslation('meetings')
  const { paragraph, words } = props
  const pieces: ReactNode[] = []

  for (let index = paragraph.start; index <= paragraph.end; index += 1) {
    if (within(props.editing, index)) {
      if (index === props.editing.start && props.editor) {
        pieces.push(<span key='editor' className='block py-2'>{props.editor}</span>)
      }
      continue
    }
    const word = pieceAttributes(props, index, false)
    pieces.push(<span
      key={words[index].id}
      data-word-index={index}
      className='transcript-piece'
      style={word.style}
      {...word.attributes}
    >{
        words[index].text
      }</span>)
    if (index < paragraph.end && !within(props.editing, index + 1)) {
      const gap = pieceAttributes(props, index, true)
      pieces.push(<span
        key={`gap-${words[index].id}`}
        data-gap-after={index}
        className='transcript-piece'
        style={gap.style}
        {...gap.attributes}
      >{' '}</span>)
    }
  }

  // A paragraph the edit covers whole leaves its words to the editor above it.
  if (!pieces.length) {
    return null
  }

  return <div
    className={[
      'relative pl-[calc(var(--excerpt-lanes)*0.5rem+0.75rem)]',
      paragraph.opensTurn
        ? 'mt-5 first:mt-0'
        : 'mt-2',
    ].join(' ')}
    style={{ '--excerpt-lanes': props.laneCount } as CSSProperties}
  >
    {props.bars?.map((bar) => <Tooltip key={bar.markId} delay={300}>
      <AriaButton
        aria-label={props.excerptLabels.get(bar.markId) ?? t('review.marks.excerpt')}
        className='absolute top-1 bottom-1 w-1 rounded-full bg-(--mark-excerpt) outline-none
          focus-visible:ring-2 focus-visible:ring-focus'
        style={{ left: `${bar.lane * EXCERPT_LANE_REM}rem` }}
        onPress={() => props.onExcerptPress(bar.markId)}
      />
      <Tooltip.Content placement='right' className='max-w-xs break-normal'>{
        props.excerptLabels.get(bar.markId) ?? t('review.marks.excerpt')
      }</Tooltip.Content>
    </Tooltip>)}
    {paragraph.opensTurn && <p className='mb-1 text-xs font-semibold'>
      <span>{paragraph.speaker || t('review.noSpeaker')}</span>
      <span className='ml-2 font-normal opacity-60'>{formatMediaPosition(words[paragraph.start].startMs)}</span>
    </p>}
    <div className='leading-8'>{pieces}</div>
  </div>
}, isSameParagraph)
