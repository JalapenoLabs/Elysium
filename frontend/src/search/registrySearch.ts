// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'
import type { TranslationNamespace } from '../i18n'
import type { SearchableText } from './searchMatch'
import type { SearchActionContext, SearchEntry, SearchResult, SearchViewer } from './searchTypes'

// Misc
import { scoreSearchMatch, toSearchWords } from './searchMatch'

// An entry in the viewer's language, its words already split for matching.
export type PreparedSearchEntry = {
  entry: SearchEntry
  title: string
  detail?: string
  text: SearchableText
}

// Translates and splits the entries that apply to the viewer, once per language, role, and
// account state, so a keystroke only compares words. An entry for admins never reaches
// anyone else's results, and Add 2FA never reaches someone who has it on.
export function prepareSearchEntries(
  entries: SearchEntry[],
  t: TFunction<TranslationNamespace[]>,
  viewer: SearchViewer,
) {
  const prepared: PreparedSearchEntry[] = []
  for (const entry of entries) {
    if (!viewer.audiences[entry.audience ?? 'everyone']) {
      continue
    }
    if (entry.when && !viewer.conditions[entry.when]) {
      continue
    }

    const title = t(entry.title)
    const detail = entry.parent && t(entry.parent)
    prepared.push({
      entry,
      title,
      detail,
      text: {
        titleWords: toSearchWords(title),
        tagWords: entry.tags
          ? toSearchWords(t(entry.tags))
          : [],
        parentWords: detail
          ? toSearchWords(detail)
          : [],
      },
    })
  }
  return prepared
}

// The prepared entries matching the query, unranked; an action runs with the context given.
export function findRegistryResults(
  prepared: PreparedSearchEntry[],
  query: string,
  context: SearchActionContext,
) {
  const queryWords = toSearchWords(query)
  const results: SearchResult[] = []
  for (const candidate of prepared) {
    const score = scoreSearchMatch(queryWords, candidate.text)
    if (score === null) {
      continue
    }

    const entry = candidate.entry
    results.push({
      id: `registry:${entry.id}`,
      group: entry.group,
      title: candidate.title,
      detail: candidate.detail,
      icon: entry.icon,
      score,
      target: 'href' in entry
        ? { kind: 'link', href: entry.href }
        : { kind: 'action', run: () => entry.run(context) },
    })
  }
  return results
}
