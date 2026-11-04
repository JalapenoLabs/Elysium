// Copyright © 2026 Jalapeno Labs

// Utility
import { diffLines } from 'diff'

// How many unchanged lines a diff keeps beside each change; the rest fold into one row.
export const DIFF_CONTEXT_LINES = 2

export type DiffRow =
  | { kind: 'added' | 'removed' | 'unchanged', text: string }
  // Unchanged lines folded away, counted.
  | { kind: 'folded', count: number }

// The lines that differ between `before` and `after`, as a reviewer reads them: every added and
// removed line, with a few unchanged ones around each change and the rest folded, so a change
// to one line of a long section reads as that line.
export function diffRows(before: string, after: string, context = DIFF_CONTEXT_LINES): DiffRow[] {
  const lines: { kind: 'added' | 'removed' | 'unchanged', text: string }[] = []
  for (const part of diffLines(before, after)) {
    const kind = part.added
      ? 'added'
      : part.removed
        ? 'removed'
        : 'unchanged'
    // A part's text ends with its last line's newline, which is not a line of its own.
    for (const text of part.value.replace(/\n$/, '').split('\n')) {
      lines.push({ kind, text })
    }
  }

  const rows: DiffRow[] = []
  let index = 0
  while (index < lines.length) {
    if (lines[index].kind !== 'unchanged') {
      rows.push(lines[index])
      index += 1
      continue
    }
    let end = index
    while (end < lines.length && lines[end].kind === 'unchanged') {
      end += 1
    }
    // Context is kept after the change before the run, and before the change after it.
    const keepFirst = index === 0
      ? 0
      : context
    const keepLast = end === lines.length
      ? 0
      : context
    const run = lines.slice(index, end)
    if (run.length <= keepFirst + keepLast) {
      rows.push(...run)
    }
    else {
      rows.push(...run.slice(0, keepFirst))
      rows.push({ kind: 'folded', count: run.length - keepFirst - keepLast })
      rows.push(...run.slice(run.length - keepLast))
    }
    index = end
  }
  return rows
}
