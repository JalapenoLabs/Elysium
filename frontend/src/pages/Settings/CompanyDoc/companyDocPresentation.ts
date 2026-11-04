// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type {
  CompanyDocAgent,
  CompanyDocHistoryEntry,
  CompanyDocSectionKey,
} from '../../../api/routes/companyDocRoutes'

// Misc
import { COMPANY_DOC_SECTIONS } from '../../../api/routes/companyDocRoutes'

export const sectionTitleKeys = {
  company: 'sections.company',
  leadership: 'sections.leadership',
  people: 'sections.people',
  stakeholders: 'sections.stakeholders',
  products: 'sections.products',
  projects: 'sections.projects',
  glossary: 'sections.glossary',
} as const satisfies Record<CompanyDocSectionKey, ParseKeys<'companyDoc'>>

export const sectionHintKeys = {
  company: 'hints.company',
  leadership: 'hints.leadership',
  people: 'hints.people',
  stakeholders: 'hints.stakeholders',
  products: 'hints.products',
  projects: 'hints.projects',
  glossary: 'hints.glossary',
} as const satisfies Record<CompanyDocSectionKey, ParseKeys<'companyDoc'>>

const HISTORY_KINDS = [ 'edited', 'approved' ] as const
type HistoryKind = typeof HISTORY_KINDS[number]

export const historySentenceKeys = {
  edited: 'history.entries.edited',
  approved: 'history.entries.approved',
} as const satisfies Record<HistoryKind, ParseKeys<'companyDoc'>>

export type HistoryDescription = {
  // `null` for a kind a later API writes.
  kind: HistoryKind | null
  // The sections it changed, in the order the doc is read.
  sections: CompanyDocSectionKey[]
  // Whether an undo wrote it.
  undid: boolean
}

// What a history entry did, read defensively: `data` is whatever the API stored, so an entry of
// a kind or shape this app does not know still reads as something.
export function describeHistoryEntry(entry: CompanyDocHistoryEntry): HistoryDescription {
  const kind = HISTORY_KINDS.find((known) => known === entry.kind) ?? null
  const changes = entry.data.changes
  const changed = typeof changes === 'object' && changes !== null
    ? Object.keys(changes)
    : []
  return {
    kind,
    sections: COMPANY_DOC_SECTIONS.filter((section) => changed.includes(section)),
    undid: typeof entry.data.undid === 'string',
  }
}

// How a proposal names the agent acting for whoever proposed it: an MCP client by the name it
// registered, a session by its number.
export type AgentLabel =
  | { kind: 'client', name: string | null }
  | { kind: 'session', number: string }

export function describeAgent(agent: CompanyDocAgent): AgentLabel {
  const session = /^session:(\d+)$/.exec(agent.id)
  if (session) {
    return { kind: 'session', number: session[1] }
  }
  return { kind: 'client', name: agent.name }
}
