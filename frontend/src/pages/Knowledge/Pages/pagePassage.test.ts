// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { findPassageRange, readPassageParam } from './pagePassage'

function textAt(body: string, range: ReturnType<typeof findPassageRange>) {
  return range && body.slice(range.start, range.end)
}

describe('findPassageRange', () => {
  it('finds a passage the body holds word for word', () => {
    const body = 'One.\n\nTwo.\n\nThree.'

    expect(textAt(body, findPassageRange(body, 'Two.\n\nThree.'))).toBe('Two.\n\nThree.')
  })

  it('reads through the whitespace the index collapses between paragraphs', () => {
    const body = '  One.\n\n\n\n   Two.\n\nThree.'

    expect(textAt(body, findPassageRange(body, 'One.\n\nTwo.'))).toBe('One.\n\n\n\n   Two.')
  })

  it('reads across a long paragraph the index cut in two at a space', () => {
    const body = 'first second'

    expect(textAt(body, findPassageRange(body, 'first\n\nsecond'))).toBe('first second')
  })

  it('keeps what the body holds of a passage that runs on into a learning\'s evidence', () => {
    const body = 'Restart it.'

    expect(textAt(body, findPassageRange(body, 'Restart it.\n\nIt came back.'))).toBe('Restart it.')
  })

  it('tells a paragraph said twice apart by what follows it', () => {
    const body = 'Note.\n\nFirst.\n\nNote.\n\nSecond.'

    const range = findPassageRange(body, 'Note.\n\nSecond.')

    expect(range).toEqual({ start: 15, end: body.length })
  })

  it('finds nothing for text the body no longer holds, or a passage of nothing', () => {
    expect(findPassageRange('One.', 'Two.')).toBeNull()
    expect(findPassageRange('One.', '')).toBeNull()
  })
})

describe('readPassageParam', () => {
  it('reads a place in a page', () => {
    expect(readPassageParam(new URLSearchParams('passage=3'))).toBe(3)
    expect(readPassageParam(new URLSearchParams('passage=0'))).toBe(0)
  })

  it('ignores what cannot be one', () => {
    expect(readPassageParam(new URLSearchParams())).toBeNull()
    expect(readPassageParam(new URLSearchParams('passage=x'))).toBeNull()
    expect(readPassageParam(new URLSearchParams('passage=-1'))).toBeNull()
    expect(readPassageParam(new URLSearchParams('passage=1.5'))).toBeNull()
  })
})
