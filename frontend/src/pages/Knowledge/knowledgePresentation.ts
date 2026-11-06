// Copyright © 2026 Jalapeno Labs

import type { ParseKeys, TFunction } from 'i18next'
import type { KnowledgePassage } from '../../api/routes/knowledgeRoutes'
import type { KnowledgeScope } from './knowledgeFilters'
import type { IconType } from 'react-icons'

// User interface
import { LuBuilding2, LuFileText, LuLightbulb, LuSend, LuSquareTerminal, LuVideo } from 'react-icons/lu'

// Misc
import { toSearchWords } from '../../search/searchMatch'
import { connectorLogos } from '../Settings/Connectors/connectorLogos'

// Every source the index holds documents of today, as `knowledge_documents.source` names them
// (docs/knowledge.md, What is indexed), in the order the filters offer them: mail, chat,
// pages and issues, meetings, then what lives in Elysium, its agents' turns first.
export const KNOWLEDGE_SOURCES = [
  'outlook',
  'gmail',
  'mail',
  'slack',
  'teams',
  'confluence',
  'youtrack',
  'meeting',
  'turn_summary',
  'page',
  'learning',
  'company_doc',
] as const
export type KnowledgeSource = typeof KNOWLEDGE_SOURCES[number]

// Everything that reads as a page: a learning and a company doc section sit in the tree and open
// as pages, whatever the index calls them, so a search of pages finds them, and offers to leave
// them out.
export const PAGE_SOURCES = [
  'page',
  'learning',
  'company_doc',
] as const satisfies readonly KnowledgeSource[]

type SourcePresentation = {
  labelKey: ParseKeys<'knowledge'>
  icon: IconType
  // A brand's own color for a connector's logo; Elysium's own sources follow the theme.
  className: string
}

const presentationBySource = {
  outlook: { labelKey: 'sources.outlook', ...connectorLogos.outlook },
  gmail: { labelKey: 'sources.gmail', ...connectorLogos.gmail },
  mail: { labelKey: 'sources.mail', icon: LuSend, className: '' },
  slack: { labelKey: 'sources.slack', ...connectorLogos.slack },
  teams: { labelKey: 'sources.teams', ...connectorLogos.teams },
  confluence: { labelKey: 'sources.confluence', ...connectorLogos.confluence },
  youtrack: { labelKey: 'sources.youtrack', ...connectorLogos.youtrack },
  meeting: { labelKey: 'sources.meeting', icon: LuVideo, className: '' },
  turn_summary: { labelKey: 'sources.turn_summary', icon: LuSquareTerminal, className: '' },
  page: { labelKey: 'sources.page', icon: LuFileText, className: '' },
  learning: { labelKey: 'sources.learning', icon: LuLightbulb, className: '' },
  company_doc: { labelKey: 'sources.company_doc', icon: LuBuilding2, className: '' },
} as const satisfies Record<KnowledgeSource, SourcePresentation>

type FilterWords = {
  sources: readonly KnowledgeSource[]
  sourcesLabel: ParseKeys<'knowledge'>
  person: ParseKeys<'knowledge'>
  personTag: ParseKeys<'knowledge'>
  since: ParseKeys<'knowledge'>
  sinceTag: ParseKeys<'knowledge'>
  until: ParseKeys<'knowledge'>
  untilTag: ParseKeys<'knowledge'>
}

// What each scope's filters offer and call themselves. A page is attributed to, and dated by, its
// newest version, so a search of pages says so in the words of its byline. The filters behind them
// are the same two.
export const filterWordsByScope = {
  everything: {
    sources: KNOWLEDGE_SOURCES,
    sourcesLabel: 'filters.sources',
    person: 'filters.person',
    personTag: 'filters.tags.person',
    since: 'filters.since',
    sinceTag: 'filters.tags.since',
    until: 'filters.until',
    untilTag: 'filters.tags.until',
  },
  pages: {
    sources: PAGE_SOURCES,
    sourcesLabel: 'filters.kinds',
    person: 'filters.lastEditedBy',
    personTag: 'filters.tags.lastEditedBy',
    since: 'filters.editedSince',
    sinceTag: 'filters.tags.editedSince',
    until: 'filters.editedUntil',
    untilTag: 'filters.tags.editedUntil',
  },
} as const satisfies Record<KnowledgeScope, FilterWords>

// A passage's text as a page shows it under its document's title. A document's text begins
// with its title (docs/knowledge.md, Timeline records), which the page already shows above it,
// so the first passage leaves it out; every other passage is shown as it is.
export function withoutTitle(passage: KnowledgePassage, title: string) {
  const leading = `${title}\n\n`
  if (passage.position !== 0 || !title || !passage.content.startsWith(leading)) {
    return passage.content
  }
  return passage.content.slice(leading.length)
}

// How many characters of a line stay before the first word that matched in it, so it still reads as
// the end of a sentence rather than starting on the match. About half a line of a result's card.
const EXCERPT_LEAD_CHARACTERS = 60

// A passage from the line that holds the most of what was searched for, since a result shows only
// its first few lines and a passage runs to 1,600 characters. A long line starts a little before
// its first match. A passage found by its meaning holds none of the words, and reads from its start.
export function excerptPassage(text: string, query: string) {
  const words = toSearchWords(query)
  const lines = text.split('\n')

  let best = { start: 0, firstMatch: -1, matches: 0 }
  let lineStart = 0
  for (const line of lines) {
    const lowered = line.toLowerCase()
    const positions = words
      .map((word) => lowered.indexOf(word))
      .filter((position) => position !== -1)
    if (positions.length > best.matches) {
      best = { start: lineStart, firstMatch: Math.min(...positions), matches: positions.length }
    }
    lineStart += line.length + 1
  }

  if (best.firstMatch > EXCERPT_LEAD_CHARACTERS) {
    const nextWord = text.indexOf(' ', best.start + best.firstMatch - EXCERPT_LEAD_CHARACTERS) + 1
    return `… ${text.slice(nextWord)}`
  }
  if (best.start) {
    return `… ${text.slice(best.start)}`
  }
  return text
}

function isKnowledgeSource(source: string): source is KnowledgeSource {
  return Object.hasOwn(presentationBySource, source)
}

// A source's name and icon. One this build does not know yet, which a newer API may index,
// reads as its own name with a plain document's icon.
export function presentKnowledgeSource(source: string, t: TFunction<'knowledge'>) {
  if (!isKnowledgeSource(source)) {
    console.debug('presentKnowledgeSource met a source it does not know', { source })
    return { label: source, icon: LuFileText, className: '' } as const
  }
  const presentation = presentationBySource[source]
  return {
    label: t(presentation.labelKey),
    icon: presentation.icon,
    className: presentation.className,
  } as const
}
