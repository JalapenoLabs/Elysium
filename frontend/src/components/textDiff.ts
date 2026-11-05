// Copyright © 2026 Jalapeno Labs

// Utility
import { diffLines, diffWordsWithSpace } from 'diff'

// How many unchanged lines a diff keeps beside each change; the rest fold into one row.
export const DIFF_CONTEXT_LINES = 2

// How long a line can be and still have its changed words marked. It takes in the longest paragraph
// anyone writes; past it the work grows quickly and nobody is reading word by word.
const WORD_DIFF_MAX_CHARACTERS = 5_000

// How much of a changed line must have stayed for its changed words to be worth marking. Below it
// the two lines are simply different lines, and marking nearly every word says nothing.
const WORD_DIFF_MIN_SHARED = 0.4

// A run of a changed line, marked when it is what changed.
export type DiffWords = { text: string, isChanged: boolean }

export type DiffRow =
  | { kind: 'added' | 'removed' | 'unchanged', text: string, words?: DiffWords[] }
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

// The runs of a line replaced by another, for each of the two, or null when too little of them is
// shared to be worth marking.
function diffChangedWords(removed: string, added: string) {
  if (Math.max(removed.length, added.length) > WORD_DIFF_MAX_CHARACTERS) {
    return null
  }

  const removedWords: DiffWords[] = []
  const addedWords: DiffWords[] = []
  let sharedCharacters = 0
  for (const part of diffWordsWithSpace(removed, added)) {
    if (part.removed) {
      removedWords.push({ text: part.value, isChanged: true })
      continue
    }
    if (part.added) {
      addedWords.push({ text: part.value, isChanged: true })
      continue
    }
    sharedCharacters += part.value.length
    removedWords.push({ text: part.value, isChanged: false })
    addedWords.push({ text: part.value, isChanged: false })
  }

  if (sharedCharacters < WORD_DIFF_MIN_SHARED * Math.max(removed.length, added.length)) {
    return null
  }
  return { removedWords, addedWords }
}

// Marks what changed inside lines replaced one for one. Markdown keeps a paragraph on one line, so
// a word changed in it reads as two whole paragraphs, red then green, that look the same until the
// words are marked. A block replaced by a different number of lines pairs from its top.
export function markChangedWords(rows: DiffRow[]): DiffRow[] {
  const marked = [ ...rows ]
  let index = 0
  while (index < marked.length) {
    let removedEnd = index
    while (marked[removedEnd]?.kind === 'removed') {
      removedEnd += 1
    }
    let addedEnd = removedEnd
    while (marked[addedEnd]?.kind === 'added') {
      addedEnd += 1
    }

    const pairs = Math.min(removedEnd - index, addedEnd - removedEnd)
    for (let offset = 0; offset < pairs; offset += 1) {
      const removed = marked[index + offset]
      const added = marked[removedEnd + offset]
      if (removed.kind === 'folded' || added.kind === 'folded') {
        continue
      }
      const words = diffChangedWords(removed.text, added.text)
      if (words) {
        marked[index + offset] = { ...removed, words: words.removedWords }
        marked[removedEnd + offset] = { ...added, words: words.addedWords }
      }
    }
    index = Math.max(addedEnd, index + 1)
  }
  return marked
}
