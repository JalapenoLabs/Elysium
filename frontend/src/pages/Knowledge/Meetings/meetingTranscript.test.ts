// Copyright © 2026 Jalapeno Labs

import type { MeetingMark, MeetingWord, MeetingWords } from '../../../api/routes/meetingRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import {
  alternativesFor,
  applyTranscriptChange,
  decorateWords,
  DRAWN_MARK_KINDS,
  excerptBarsByParagraph,
  excerptLanes,
  markKindBits,
  marksUnder,
  paragraphContaining,
  paragraphsOf,
  placeMarks,
  rangeBetween,
  summarizeRange,
  textOfRange,
} from './meetingTranscript'

const ALL_SHOWN = new Set(DRAWN_MARK_KINDS)

function wordsOf(spoken: [ string, string ][]): MeetingWord[] {
  return spoken.map(([ speaker, text ], index) => ({
    id: `w${index}`,
    position: (index + 1) * 1024,
    text,
    speaker,
    startMs: index * 500,
    endMs: index * 500 + 500,
    origin: 'source',
    captureId: 'capture',
  }))
}

function markOf(id: string, kind: string, startWordId: string, endWordId: string, data = {}): MeetingMark {
  return {
    id,
    kind,
    startWordId,
    endWordId,
    data,
    createdBy: 'ada',
    agent: null,
    createdAt: '2026-11-10T12:00:00Z',
    excerpt: null,
  }
}

const words = wordsOf([
  [ 'Ada', 'Launch' ],
  [ 'Ada', 'is' ],
  [ 'Ada', 'Fryday.' ],
  [ 'Grace', 'Agreed.' ],
])

describe('applyTranscriptChange', () => {
  const transcript: MeetingWords = { words, marks: [], captures: []}
  const edit = {
    removedWordIds: [ 'w2' ],
    words: [
      { ...words[2], id: 'n2', position: 3073, text: 'Friday.' },
      { ...words[2], id: 'n1', position: 3072, text: 'on' },
    ],
    marks: [ markOf('edit', 'edit', 'n1', 'n2') ],
  }

  it('takes out what a change replaced and stands its words in their place', () => {
    const changed = applyTranscriptChange(transcript, edit)
    expect(changed.words.map((word) => word.text)).toEqual([ 'Launch', 'is', 'on', 'Friday.', 'Agreed.' ])
    expect(changed.marks).toEqual(edit.marks)
  })

  it('changes nothing more when the same change arrives again', () => {
    const once = applyTranscriptChange(transcript, edit)
    const twice = applyTranscriptChange(once, edit)
    expect(twice.words).toEqual(once.words)
  })

  it('puts words standing again after the last word', () => {
    const appended = applyTranscriptChange(transcript, {
      removedWordIds: [],
      words: [{ ...words[3], id: 'tail', position: 99_999, text: 'Done.' }],
      marks: [],
    })
    expect(appended.words.at(-1)?.text).toBe('Done.')
  })
})

describe('placeMarks', () => {
  it('places drawn marks by index, longest first where they start together', () => {
    const placed = placeMarks(words, [
      markOf('short', 'comment', 'w0', 'w0'),
      markOf('long', 'excerpt', 'w0', 'w2'),
      markOf('later', 'confidence', 'w3', 'w3', { confidence: 10 }),
    ])
    expect(placed.map((placedMark) => [ placedMark.mark.id, placedMark.start, placedMark.end ])).toEqual([
      [ 'long', 0, 2 ],
      [ 'short', 0, 0 ],
      [ 'later', 3, 3 ],
    ])
  })

  it('leaves out kinds it does not draw and marks over words that do not stand', () => {
    const placed = placeMarks(words, [
      markOf('pin', 'pin', 'w1', 'w1'),
      markOf('gone', 'edit', 'w1', 'replaced'),
      markOf('backwards', 'edit', 'w2', 'w1'),
    ])
    expect(placed).toEqual([])
  })
})

describe('paragraphsOf', () => {
  it('opens a paragraph, naming the speaker, wherever the speaker changes', () => {
    expect(paragraphsOf(words)).toEqual([
      { speaker: 'Ada', start: 0, end: 2, opensTurn: true },
      { speaker: 'Grace', start: 3, end: 3, opensTurn: true },
    ])
  })

  it('breaks a long turn at a sentence end, continuing the same speaker', () => {
    const long = wordsOf(Array.from({ length: 90 }, (_, index): [ string, string ] => [
      'Ada',
      index === 84
        ? 'end.'
        : 'word',
    ]))
    const paragraphs = paragraphsOf(long)
    expect(paragraphs).toEqual([
      { speaker: 'Ada', start: 0, end: 84, opensTurn: true },
      { speaker: 'Ada', start: 85, end: 89, opensTurn: false },
    ])
    expect(paragraphContaining(paragraphs, 84)).toBe(0)
    expect(paragraphContaining(paragraphs, 85)).toBe(1)
  })

  it('breaks a turn with no sentence end once it is very long', () => {
    const paragraphs = paragraphsOf(wordsOf(Array.from({ length: 450 }, (): [ string, string ] => [ 'Ada', 'word' ])))
    expect(paragraphs.map((paragraph) => paragraph.end - paragraph.start + 1)).toEqual([ 200, 200, 50 ])
  })
})

describe('decorateWords', () => {
  const placed = placeMarks(words, [
    markOf('unsure', 'confidence', 'w1', 'w2', { confidence: 40 }),
    markOf('very unsure', 'confidence', 'w2', 'w2', { confidence: 5 }),
    markOf('edited', 'edit', 'w0', 'w1'),
  ])

  it('covers each word and the spaces inside a mark, with the lowest confidence over each', () => {
    const decorations = decorateWords(words.length, placed, ALL_SHOWN)
    expect(Array.from(decorations.covered)).toEqual([
      markKindBits.edit,
      markKindBits.edit | markKindBits.confidence,
      markKindBits.confidence,
      0,
    ])
    // The space after a mark's last word is outside it.
    expect(Array.from(decorations.gapCovered)).toEqual([ markKindBits.edit, markKindBits.confidence, 0, 0 ])
    expect(decorations.confidence).toEqual([ null, 40, 5, null ])
    expect(decorations.gapConfidence).toEqual([ null, 40, null, null ])
    expect(decorations.starts[0]).toBe(markKindBits.edit)
    expect(decorations.ends[1]).toBe(markKindBits.edit)
  })

  it('leaves out the kinds switched off', () => {
    const decorations = decorateWords(words.length, placed, new Set([ 'edit' ] as const))
    expect(decorations.confidence).toEqual([ null, null, null, null ])
    expect(decorations.covered[2]).toBe(0)
  })
})

describe('excerptLanes', () => {
  it('stands overlapping excerpts side by side and reuses a lane once it is free', () => {
    const placed = placeMarks(words, [
      markOf('topic', 'excerpt', 'w0', 'w2'),
      markOf('nested', 'excerpt', 'w1', 'w1'),
      markOf('after', 'excerpt', 'w3', 'w3'),
    ])
    const lanes = excerptLanes(placed)
    expect(Object.fromEntries(lanes)).toEqual({ topic: 0, nested: 1, after: 0 })

    const bars = excerptBarsByParagraph(paragraphsOf(words), placed, lanes)
    expect(bars.get(0)).toEqual([{ markId: 'topic', lane: 0 }, { markId: 'nested', lane: 1 }])
    expect(bars.get(1)).toEqual([{ markId: 'after', lane: 0 }])
  })
})

describe('marksUnder', () => {
  it('answers the marks over a word among those shown, shortest first', () => {
    const placed = placeMarks(words, [
      markOf('topic', 'excerpt', 'w0', 'w3'),
      markOf('note', 'comment', 'w1', 'w2'),
      markOf('edited', 'edit', 'w1', 'w1'),
    ])
    expect(marksUnder(placed, 1, ALL_SHOWN).map((placedMark) => placedMark.mark.id)).toEqual([
      'edited',
      'note',
      'topic',
    ])
    expect(marksUnder(placed, 1, new Set([ 'excerpt' ] as const)).map((placedMark) => placedMark.mark.id)).toEqual([
      'topic',
    ])
    expect(marksUnder(placed, 3, new Set([ 'comment' ] as const))).toEqual([])
  })
})

describe('alternativesFor', () => {
  it('offers the alternatives of confidence marks over exactly the range, once each', () => {
    const placed = placeMarks(words, [
      markOf('flag', 'confidence', 'w2', 'w2', { confidence: 20, alternatives: [ 'Friday.', ' ', 'Fri day.' ]}),
      markOf('again', 'confidence', 'w2', 'w2', { confidence: 30, alternatives: [ 'Friday.', 7 ]}),
      markOf('wider', 'confidence', 'w1', 'w2', { confidence: 30, alternatives: [ 'is Friday.' ]}),
    ])
    expect(alternativesFor(placed, { start: 2, end: 2 })).toEqual([ 'Friday.', 'Fri day.' ])
    expect(alternativesFor(placed, { start: 0, end: 0 })).toEqual([])
  })
})

describe('ranges', () => {
  it('reads a range either way round, its text, and its span on the meeting clock', () => {
    const range = rangeBetween(2, 0)
    expect(range).toEqual({ start: 0, end: 2 })
    expect(textOfRange(words, range)).toBe('Launch is Fryday.')
    expect(summarizeRange(words, range)).toEqual({ startMs: 0, endMs: 1500, wordCount: 3 })
  })
})
