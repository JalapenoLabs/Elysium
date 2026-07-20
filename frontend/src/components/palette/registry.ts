// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

// Core
import { useEffect } from 'react'

// Utility
import { fuzzyMatch } from '@jalapenolabs/uikit'

// Misc
import { createRegistry } from '../../lib/registry'

// The command-palette registry (docs/ux.md, keyboard-first): any pillar can
// contribute entries (navigation targets, actions) without the palette knowing
// it exists. Registration is grouped so a pillar's entries replace and retire
// together, and the palette re-renders through the subscription.
export type PaletteEntry = {
  id: string
  // Already-translated display text. Register from inside React (see
  // usePaletteEntries) so a language change re-registers translated labels.
  label: string
  // Extra translated text the fuzzy matcher may hit (synonyms, paths).
  keywords?: string
  // A short translated group heading shown above the entry.
  section?: string
  icon?: ReactNode
  run: () => void
}

export type RankedPaletteEntry = {
  entry: PaletteEntry
  score: number
  // Indices into entry.label when the label matched, for highlight rendering.
  labelIndices: number[]
}

const registry = createRegistry<PaletteEntry>()

export function registerPaletteEntries(groupId: string, entries: PaletteEntry[]): () => void {
  return registry.register(groupId, entries)
}

export const listPaletteEntries = registry.list
export const subscribePalette = registry.subscribe

// Registers a group of entries for the lifetime of the calling component.
// `entries` must be referentially stable (useMemo) or the group re-registers
// every render, which is harmless but wasteful.
export function usePaletteEntries(groupId: string, entries: PaletteEntry[]) {
  useEffect(() => {
    return registry.register(groupId, entries)
  }, [ groupId, entries ])
}

// Ranks entries against a query with uikit's subsequence matcher. An empty
// query returns everything unranked (browse mode); otherwise an entry matches
// when its label or keywords do, best label scores first.
export function rankPaletteEntries(query: string, entries: PaletteEntry[]): RankedPaletteEntry[] {
  const trimmedQuery = query.trim()
  if (!trimmedQuery) {
    return entries.map((entry) => ({
      entry,
      score: 0,
      labelIndices: [],
    }))
  }

  const ranked: RankedPaletteEntry[] = []
  for (const entry of entries) {
    const labelMatch = fuzzyMatch(trimmedQuery, entry.label)
    const keywordMatch = entry.keywords
      ? fuzzyMatch(trimmedQuery, entry.keywords)
      : null

    if (!labelMatch && !keywordMatch) {
      continue
    }

    ranked.push({
      entry,
      score: Math.max(labelMatch?.score ?? 0, keywordMatch?.score ?? 0),
      labelIndices: labelMatch?.indices ?? [],
    })
  }

  return ranked.sort((first, second) => second.score - first.score)
}
