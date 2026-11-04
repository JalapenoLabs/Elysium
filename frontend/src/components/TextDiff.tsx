// Copyright © 2026 Jalapeno Labs

// Core
import { useMemo } from 'react'

// Misc
import { diffRows } from './textDiff'

type Props = {
  before: string
  after: string
  // What a row of folded, unchanged lines says, given how many it holds.
  foldedLabel: (count: number) => string
  // Names the diff for screen readers, such as which section it changes.
  label: string
}

const rowClasses = {
  added: 'bg-success/10 text-success',
  removed: 'bg-danger/10 text-danger',
  unchanged: 'opacity-70',
} as const

const rowMarks = {
  added: '+',
  removed: '−',
  unchanged: ' ',
} as const

// Two versions of a text, line by line, as a reviewer reads a change: added lines green, removed
// lines red, a few unchanged lines around each change, and the rest folded.
export function TextDiff(props: Props) {
  const rows = useMemo(() => diffRows(props.before, props.after), [ props.before, props.after ])

  return <div
    role='group'
    aria-label={props.label}
    className='overflow-x-auto rounded-lg bg-surface-secondary font-mono text-xs'
  >{
      rows.map((row, index) => {
        if (row.kind === 'folded') {
          return <div key={index} className='px-3 py-1 italic opacity-50'>{
            props.foldedLabel(row.count)
          }</div>
        }
        return <div key={index} className={`flex gap-2 px-3 whitespace-pre-wrap ${rowClasses[row.kind]}`}>
          <span aria-hidden className='select-none'>{rowMarks[row.kind]}</span>
          <span className='min-w-0 flex-1'>{row.text || ' '}</span>
        </div>
      })
    }</div>
}
