// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { diffRows, markChangedWords } from './textDiff'

describe('diffRows', () => {
  it('shows an added line beside the lines around it', () => {
    const rows = diffRows('- Alpha\n- Delta', '- Alpha\n- Bravo\n- Delta')

    expect(rows).toEqual([
      { kind: 'unchanged', text: '- Alpha' },
      { kind: 'added', text: '- Bravo' },
      { kind: 'unchanged', text: '- Delta' },
    ])
  })

  it('shows a changed line as the line removed and the line added', () => {
    const rows = diffRows('- Priya Ramen', '- Priya Raman')

    expect(rows).toEqual([
      { kind: 'removed', text: '- Priya Ramen' },
      { kind: 'added', text: '- Priya Raman' },
    ])
  })

  it('folds the unchanged lines of a long section, keeping a little context by each change', () => {
    const before = Array.from({ length: 20 }, (_, index) => `line ${index}`).join('\n')
    const after = before.replace('line 10', 'line ten')

    const rows = diffRows(before, after)

    expect(rows).toEqual([
      { kind: 'folded', count: 8 },
      { kind: 'unchanged', text: 'line 8' },
      { kind: 'unchanged', text: 'line 9' },
      { kind: 'removed', text: 'line 10' },
      { kind: 'added', text: 'line ten' },
      { kind: 'unchanged', text: 'line 11' },
      { kind: 'unchanged', text: 'line 12' },
      { kind: 'folded', count: 7 },
    ])
  })

  it('shows a section written from nothing as every line added', () => {
    expect(diffRows('', '- ACME\n- Atlas')).toEqual([
      { kind: 'added', text: '- ACME' },
      { kind: 'added', text: '- Atlas' },
    ])
  })
})

describe('markChangedWords', () => {
  it('marks the words a line changed, on both sides', () => {
    const rows = markChangedWords(diffRows(
      'A worker renews its lease and holds its slot.',
      'A worker renews its lease often and holds its slot.',
    ))

    expect(rows).toEqual([
      {
        kind: 'removed',
        text: 'A worker renews its lease and holds its slot.',
        words: [
          { text: 'A worker renews its lease ', isChanged: false },
          { text: 'and holds its slot.', isChanged: false },
        ],
      },
      {
        kind: 'added',
        text: 'A worker renews its lease often and holds its slot.',
        words: [
          { text: 'A worker renews its lease ', isChanged: false },
          { text: 'often ', isChanged: true },
          { text: 'and holds its slot.', isChanged: false },
        ],
      },
    ])
  })

  it('pairs a block of replaced lines from its top', () => {
    const rows = markChangedWords(diffRows(
      'The first line stays nearly the same\nThe second line stays nearly the same',
      'The first line stays almost the same\nThe second line stays almost the same\nA line of its own',
    ))

    expect(rows.map((row) => 'words' in row && row.words !== undefined)).toEqual([
      true,
      true,
      true,
      true,
      false,
    ])
  })

  it('leaves two lines that share too little whole', () => {
    const rows = markChangedWords(diffRows('Nothing here is shared', 'Every word differs entirely now'))

    expect(rows).toEqual([
      { kind: 'removed', text: 'Nothing here is shared' },
      { kind: 'added', text: 'Every word differs entirely now' },
    ])
  })

  it('leaves a line too long to read word by word whole', () => {
    const before = 'word '.repeat(2000)
    const after = `${before}and one more`

    const rows = markChangedWords(diffRows(before, after))

    expect(rows.every((row) => !('words' in row))).toBe(true)
  })

  it('leaves added, removed, and unchanged lines with no partner alone', () => {
    const rows = diffRows('kept\ngone\nkept too', 'kept\nkept too\nnew')

    expect(markChangedWords(rows)).toEqual(rows)
  })
})
