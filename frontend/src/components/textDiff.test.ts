// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { diffRows } from './textDiff'

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
