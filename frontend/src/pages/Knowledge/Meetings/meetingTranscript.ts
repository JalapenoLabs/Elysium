// Copyright © 2026 Jalapeno Labs

import type {
  MeetingMark,
  MeetingMarkKind,
  MeetingTranscriptChange,
  MeetingWord,
  MeetingWords,
} from '../../../api/routes/meetingRoutes'

// How a meeting's final transcript is laid out and marked on its page (docs/meeting-review.md,
// The meeting's page, and Marks). Everything here is pure, so the page's components only draw
// what it answers, and the tests read it directly.
//
// Words are addressed by their index in the transcript as it reads. A mark is anchored to word
// ids, which the page turns into indices once per load (`placeMarks`); a range is two indices,
// both included.

// The kinds of mark drawn over the words, in the order the options list their switches. Pins
// and tasks show along the top of the page instead (docs/meeting-review.md, Tasks, and
// Attachments and frames).
export const DRAWN_MARK_KINDS = [
  'confidence',
  'edit',
  'excerpt',
  'black_out',
  'comment',
] as const satisfies readonly MeetingMarkKind[]
export type DrawnMarkKind = typeof DRAWN_MARK_KINDS[number]

// One bit per drawn kind, so a word says every kind over it in one small number.
export const markKindBits = {
  confidence: 1,
  edit: 2,
  excerpt: 4,
  black_out: 8,
  comment: 16,
} as const satisfies Record<DrawnMarkKind, number>

// The most words a paragraph holds before it breaks at the next sentence's end, and the most it
// holds at all. One speaker can talk for ten minutes, and a paragraph is what redraws when a
// selection reaches into it, so it stays small enough to redraw at once.
const PARAGRAPH_SOFT_WORDS = 80
const PARAGRAPH_HARD_WORDS = 200
const SENTENCE_END = /[.?!]["')\]]*$/

export type WordRange = {
  start: number
  end: number
}

// A mark as it lies on the transcript as it reads.
export type PlacedMark = {
  mark: MeetingMark
  kind: DrawnMarkKind
  start: number
  end: number
}

// A run of words one speaker said, as the page draws it. `opensTurn` is the first paragraph of a
// speaker's turn, which names them.
export type Paragraph = {
  speaker: string
  start: number
  end: number
  opensTurn: boolean
}

// What lies over each word, and over the space after it, for the kinds shown. A space is
// covered when one mark covers the words on both sides of it, so a mark draws as one unbroken
// box rather than a box per word.
export type WordDecorations = {
  // Bits of `markKindBits`.
  covered: Uint8Array
  gapCovered: Uint8Array
  starts: Uint8Array
  ends: Uint8Array
  // The lowest confidence over a word, or over the space after it, from 0 to 100.
  confidence: (number | null)[]
  gapConfidence: (number | null)[]
}

// An excerpt's bar in a paragraph's gutter: its lane, so overlapping excerpts stand side by side.
export type ExcerptBar = {
  markId: string
  lane: number
}

// Applies what one change answered to the transcript held, so an edit shows at once rather than
// after reading tens of thousands of words again. Applying the same change twice changes
// nothing, since the event stream's reload may bring it back.
export function applyTranscriptChange(transcript: MeetingWords, change: MeetingTranscriptChange): MeetingWords {
  const removed = new Set(change.removedWordIds)
  const kept: MeetingWord[] = []
  const keptIds = new Set<string>()
  for (const word of transcript.words) {
    if (removed.has(word.id)) {
      continue
    }
    kept.push(word)
    keptIds.add(word.id)
  }

  const added = change.words
    .filter((word) => !keptIds.has(word.id))
    .toSorted((first, second) => first.position - second.position)

  // Both lists are in order of position, so one merge places every new word.
  const words: MeetingWord[] = []
  let addedIndex = 0
  for (const word of kept) {
    while (addedIndex < added.length && added[addedIndex].position < word.position) {
      words.push(added[addedIndex])
      addedIndex += 1
    }
    words.push(word)
  }
  words.push(...added.slice(addedIndex))

  return {
    ...transcript,
    words,
    marks: change.marks,
  }
}

// Every drawn mark whose ends stand in the transcript, by index, in order of where it starts and
// the longest first. A kind this build does not draw is left out; a mark over a word that does not
// stand is a transcript and its marks read at different moments, which the next read fixes.
export function placeMarks(words: MeetingWord[], marks: MeetingMark[]): PlacedMark[] {
  const indexById = new Map<string, number>()
  for (const [ index, word ] of words.entries()) {
    indexById.set(word.id, index)
  }

  const placed: PlacedMark[] = []
  for (const mark of marks) {
    const kind = DRAWN_MARK_KINDS.find((drawn) => drawn === mark.kind)
    if (!kind) {
      continue
    }
    const start = indexById.get(mark.startWordId)
    const end = indexById.get(mark.endWordId)
    if (start === undefined || end === undefined || end < start) {
      console.debug('placeMarks found a mark over words that do not stand, leaving it out', {
        markId: mark.id,
      })
      continue
    }
    placed.push({ mark, kind, start, end })
  }
  return placed.toSorted((first, second) => first.start - second.start || second.end - first.end)
}

// The transcript cut into paragraphs: a new one where the speaker changes, and inside a long turn
// at the first sentence's end past `PARAGRAPH_SOFT_WORDS`, or at `PARAGRAPH_HARD_WORDS` whatever
// the punctuation.
export function paragraphsOf(words: MeetingWord[]): Paragraph[] {
  const paragraphs: Paragraph[] = []
  for (const [ index, word ] of words.entries()) {
    const current = paragraphs.at(-1)
    if (current && current.speaker === word.speaker) {
      const length = index - current.start
      const endsSentence = SENTENCE_END.test(words[index - 1].text)
      const isFull = length >= PARAGRAPH_HARD_WORDS || (length >= PARAGRAPH_SOFT_WORDS && endsSentence)
      if (!isFull) {
        current.end = index
        continue
      }
      paragraphs.push({ speaker: word.speaker, start: index, end: index, opensTurn: false })
      continue
    }
    paragraphs.push({ speaker: word.speaker, start: index, end: index, opensTurn: true })
  }
  return paragraphs
}

// What lies over each word for the kinds in `shown`, in one pass over each mark's words.
export function decorateWords(
  wordCount: number,
  placed: PlacedMark[],
  shown: ReadonlySet<DrawnMarkKind>,
): WordDecorations {
  const decorations: WordDecorations = {
    covered: new Uint8Array(wordCount),
    gapCovered: new Uint8Array(wordCount),
    starts: new Uint8Array(wordCount),
    ends: new Uint8Array(wordCount),
    confidence: new Array<number | null>(wordCount).fill(null),
    gapConfidence: new Array<number | null>(wordCount).fill(null),
  }

  for (const { mark, kind, start, end } of placed) {
    if (!shown.has(kind)) {
      continue
    }
    const bit = markKindBits[kind]
    const confidence = kind === 'confidence'
      ? confidenceOf(mark)
      : null
    decorations.starts[start] |= bit
    decorations.ends[end] |= bit
    for (let index = start; index <= end; index += 1) {
      decorations.covered[index] |= bit
      if (index < end) {
        decorations.gapCovered[index] |= bit
      }
      if (confidence === null) {
        continue
      }
      decorations.confidence[index] = lowest(decorations.confidence[index], confidence)
      if (index < end) {
        decorations.gapConfidence[index] = lowest(decorations.gapConfidence[index], confidence)
      }
    }
  }
  return decorations
}

function lowest(current: number | null, next: number) {
  if (current === null) {
    return next
  }
  return Math.min(current, next)
}

// A lane for every excerpt, the fewest that keep overlapping ones apart: each takes the first
// lane free where it starts.
export function excerptLanes(placed: PlacedMark[]): Map<string, number> {
  const lanes = new Map<string, number>()
  // The index each lane is taken up to.
  const laneEnds: number[] = []
  for (const excerpt of placed) {
    if (excerpt.kind !== 'excerpt') {
      continue
    }
    let lane = laneEnds.findIndex((end) => end < excerpt.start)
    if (lane === -1) {
      lane = laneEnds.length
      laneEnds.push(excerpt.end)
    }
    else {
      laneEnds[lane] = excerpt.end
    }
    lanes.set(excerpt.mark.id, lane)
  }
  return lanes
}

// The excerpt bars each paragraph draws in its gutter, by paragraph index.
export function excerptBarsByParagraph(
  paragraphs: Paragraph[],
  placed: PlacedMark[],
  lanes: Map<string, number>,
): Map<number, ExcerptBar[]> {
  const bars = new Map<number, ExcerptBar[]>()
  for (const excerpt of placed) {
    const lane = lanes.get(excerpt.mark.id)
    if (lane === undefined) {
      continue
    }
    const first = paragraphContaining(paragraphs, excerpt.start)
    for (let index = first; index < paragraphs.length && paragraphs[index].start <= excerpt.end; index += 1) {
      const list = bars.get(index) ?? []
      list.push({ markId: excerpt.mark.id, lane })
      bars.set(index, list)
    }
  }
  return bars
}

// The index of the paragraph holding word `wordIndex`, by binary search.
export function paragraphContaining(paragraphs: Paragraph[], wordIndex: number) {
  let low = 0
  let high = paragraphs.length - 1
  while (low < high) {
    const middle = Math.ceil((low + high) / 2)
    if (paragraphs[middle].start <= wordIndex) {
      low = middle
    }
    else {
      high = middle - 1
    }
  }
  return low
}

// The marks over word `index` among the kinds shown, shortest first: the one a click selects,
// then the rest as its pills, in the order Tab moves through them.
export function marksUnder(placed: PlacedMark[], index: number, shown: ReadonlySet<DrawnMarkKind>) {
  return placed
    .filter((placedMark) => shown.has(placedMark.kind)
      && placedMark.start <= index
      && index <= placedMark.end)
    .toSorted((first, second) => (first.end - first.start) - (second.end - second.start)
      || first.start - second.start)
}

// How sure a confidence mark is, from 0 to 100: `data.confidence`, held to the scale. `null` for
// a mark that says nothing a confidence can be read from.
export function confidenceOf(mark: MeetingMark) {
  const confidence = mark.data.confidence
  if (typeof confidence !== 'number' || Number.isNaN(confidence)) {
    console.debug('confidenceOf found a mark with no confidence', { markId: mark.id })
    return null
  }
  return Math.min(100, Math.max(0, confidence))
}

// What else the correcting agent thought a range might say: a confidence mark's
// `data.alternatives`, up to three non-empty strings (docs/meeting-review.md, Settling).
export function alternativesOf(mark: MeetingMark): string[] {
  const alternatives = mark.data.alternatives
  if (!Array.isArray(alternatives)) {
    return []
  }
  const texts: string[] = []
  for (const alternative of alternatives) {
    const text = typeof alternative === 'string'
      ? alternative.trim()
      : ''
    if (text && !texts.includes(text)) {
      texts.push(text)
    }
  }
  return texts
}

// The alternatives an edit of `range` offers: those of every confidence mark over exactly it.
export function alternativesFor(placed: PlacedMark[], range: WordRange): string[] {
  const choices: string[] = []
  for (const placedMark of placed) {
    if (placedMark.kind !== 'confidence' || placedMark.start !== range.start || placedMark.end !== range.end) {
      continue
    }
    for (const alternative of alternativesOf(placedMark.mark)) {
      if (!choices.includes(alternative)) {
        choices.push(alternative)
      }
    }
  }
  return choices
}

// The range from one word to another, whichever comes first.
export function rangeBetween(anchor: number, focus: number): WordRange {
  return {
    start: Math.min(anchor, focus),
    end: Math.max(anchor, focus),
  }
}

// What a range says, a space between each word.
export function textOfRange(words: MeetingWord[], range: WordRange) {
  return words
    .slice(range.start, range.end + 1)
    .map((word) => word.text)
    .join(' ')
}

export type RangeSummary = {
  startMs: number
  endMs: number
  wordCount: number
}

// Where a range starts and ends on the meeting's clock, and how many words it holds.
export function summarizeRange(words: MeetingWord[], range: WordRange): RangeSummary {
  const first = words[range.start]
  const last = words[range.end]
  return {
    startMs: first.startMs,
    endMs: Math.max(first.startMs, last.endMs),
    wordCount: range.end - range.start + 1,
  }
}
