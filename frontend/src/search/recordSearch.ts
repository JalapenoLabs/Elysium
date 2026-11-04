// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'
import type { IconType } from 'react-icons'
import type { RecordSearchHit, RecordSearchKind } from '../api/routes/searchRoutes'
import type { TranslationNamespace } from '../i18n'
import type { SearchGroup, SearchResult } from './searchTypes'

// User interface
import { LuCodeXml, LuFolderKanban, LuListTodo, LuTarget } from 'react-icons/lu'

// Misc
import {
  getActionItemViewUrl,
  getCodingSessionUrl,
  getInitiativeViewUrl,
  getProjectViewUrl,
} from '../urls'
import { scoreSearchMatch, toSearchWords } from './searchMatch'

// Each kind of record's section.
const groupByKind = {
  actionItem: 'actionItems',
  project: 'projects',
  codingSession: 'codingSessions',
  initiative: 'initiatives',
} as const satisfies Record<RecordSearchKind, SearchGroup>

// Each kind's icon, as its area shows it in the sidebar and the registry.
const iconByKind = {
  actionItem: LuListTodo,
  project: LuFolderKanban,
  codingSession: LuCodeXml,
  initiative: LuTarget,
} as const satisfies Record<RecordSearchKind, IconType>

// The records the API found, as results beside the registry's. Each is scored by its title
// with the registry's own rule, so the merged list ranks every source alike. The API matched
// it already, so one the browser reads differently (an accent, an underscore) still shows,
// ranked after every closer match.
export function toRecordResults(
  hits: RecordSearchHit[],
  query: string,
  t: TFunction<TranslationNamespace[]>,
) {
  const queryWords = toSearchWords(query)
  const results: SearchResult[] = []

  for (const hit of hits) {
    const description = describeHit(hit, t)
    if (!description) {
      console.debug('toRecordResults left out a kind of record it does not know', { hit })
      continue
    }

    const text = {
      titleWords: toSearchWords(hit.title),
      tagWords: [],
      parentWords: [],
    }
    results.push({
      id: `records:${hit.kind}:${hit.id}`,
      group: groupByKind[hit.kind],
      title: hit.title,
      icon: iconByKind[hit.kind],
      score: scoreSearchMatch(queryWords, text) ?? 0,
      ...description,
    })
  }
  return results
}

// Where a record opens, and its quieter second line. Each kind's id and details differ, so
// this reads the hit by its kind. The API's answer is data from outside the app, so a kind
// this build does not know yet is answered with nothing rather than assumed away.
function describeHit(hit: RecordSearchHit, t: TFunction<TranslationNamespace[]>) {
  switch (hit.kind) {
    case 'actionItem':
      return {
        target: { kind: 'link', href: getActionItemViewUrl(hit.id) },
        detail: t(`actionItems:states.${hit.state}`),
      } as const
    case 'project':
      return {
        target: { kind: 'link', href: getProjectViewUrl(hit.id) },
      } as const
    case 'codingSession':
      return {
        target: { kind: 'link', href: getCodingSessionUrl(hit.id) },
        detail: t('coding:sessions.numbered', { number: hit.id }),
      } as const
    case 'initiative':
      return {
        target: { kind: 'link', href: getInitiativeViewUrl(hit.id) },
        detail: t(`initiatives:states.${hit.state}`),
      } as const
    default:
      return null
  }
}
