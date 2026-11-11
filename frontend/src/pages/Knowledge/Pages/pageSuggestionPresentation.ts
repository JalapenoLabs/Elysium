// Copyright © 2026 Jalapeno Labs

import type { PageSuggestionChange } from '../../../api/routes/pageRoutes'

// What a suggestion's card starts with ticked: every change that still merges with the page. One
// whose lines the page changed since would be refused, so it is never taken by default.
export function initialChoice(changes: PageSuggestionChange[]) {
  const chosen = new Set<number>()
  for (const change of changes) {
    if (change.applies) {
      chosen.add(change.index)
    }
  }
  return chosen
}

// `chosen` with `index` ticked if it was not, and unticked if it was.
export function toggleChoice(chosen: ReadonlySet<number>, index: number) {
  const next = new Set(chosen)
  if (next.has(index)) {
    next.delete(index)
  }
  else {
    next.add(index)
  }
  return next
}

// What Accept takes and says: every change, which the API is asked for by naming none, or some of
// them, named by their index in order.
export function acceptLabel(chosen: ReadonlySet<number>, count: number) {
  if (chosen.size === count) {
    return { kind: 'all', changes: undefined } as const
  }
  const changes = [ ...chosen ].sort((first, second) => first - second)
  return { kind: 'some', changes, taken: chosen.size, count } as const
}
