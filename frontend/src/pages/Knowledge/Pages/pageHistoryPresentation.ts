// Copyright © 2026 Jalapeno Labs

import type { ParseKeys, TFunction } from 'i18next'
import type { LearningAgent } from '../../../api/routes/learningRoutes'
import type { PageHistoryEntry } from '../../../api/routes/pageRoutes'

// Misc
import { PAGE_COMPARE_FROM_PARAM, PAGE_COMPARE_TO_PARAM, PAGE_VERSION_PARAM } from '../../../urls'

export const pageHistorySentenceKeys = {
  created: 'pages.history.sentences.created',
  published: 'pages.history.sentences.published',
  restored: 'pages.history.sentences.restored',
  undid: 'pages.history.sentences.undid',
  undidEarlier: 'pages.history.sentences.undidEarlier',
  approved: 'pages.history.sentences.approved',
  moved: 'pages.history.sentences.moved',
  reordered: 'pages.history.sentences.reordered',
  deleted: 'pages.history.sentences.deleted',
  deletedWith: 'pages.history.sentences.deletedWith',
  broughtBack: 'pages.history.sentences.broughtBack',
  broughtBackWith: 'pages.history.sentences.broughtBackWith',
  labeled: 'pages.history.sentences.labeled',
  unlabeled: 'pages.history.sentences.unlabeled',
  attached: 'pages.history.sentences.attached',
  detached: 'pages.history.sentences.detached',
  projectLinked: 'pages.history.sentences.projectLinked',
  projectUnlinked: 'pages.history.sentences.projectUnlinked',
  changed: 'pages.history.sentences.changed',
} as const satisfies Record<string, ParseKeys<'knowledge'>>

type PageHistorySentence = keyof typeof pageHistorySentenceKeys

export type PageHistoryRow = {
  entry: PageHistoryEntry
  sentence: PageHistorySentence
  // The version this entry wrote; null for one that wrote none, such as a move.
  version: number | null
  // The version it copied or undid, which its sentence names.
  otherVersion: number | null
  // Where a move took it; a null parent is the top of the tree.
  move: { fromParentId: string | null, toParentId: string | null } | null
  // The label it put on or took off.
  label: string | null
  // The file it attached or took off.
  attachment: { id: string, name: string } | null
  // The project it was linked to or taken off, as it was called then.
  project: string | null
}

type EntryReading = Partial<PageHistoryRow> & Pick<PageHistoryRow, 'sentence'>

function readNumber(value: unknown) {
  return typeof value === 'number'
    ? value
    : null
}

// One side of a move, `{ parentId, after }`; undefined when it is not shaped like one.
function readParentId(side: unknown): string | null | undefined {
  if (typeof side !== 'object' || side === null || !('parentId' in side)) {
    return undefined
  }
  const { parentId } = side
  if (parentId === null || typeof parentId === 'string') {
    return parentId
  }
  return undefined
}

function readPublished(
  data: PageHistoryEntry['data'],
  versionByEntryId: Map<string, number>,
): EntryReading {
  const restoredFrom = readNumber(data.restoredFrom)
  if (restoredFrom !== null) {
    return { sentence: 'restored', otherVersion: restoredFrom }
  }
  if (typeof data.undid === 'string') {
    const undidVersion = versionByEntryId.get(data.undid)
    // An undo of a deletion or a restore reversed no version.
    if (undidVersion === undefined) {
      return { sentence: 'undidEarlier' }
    }
    return { sentence: 'undid', otherVersion: undidVersion }
  }
  if (data.proposalId) {
    return { sentence: 'approved' }
  }
  return { sentence: 'published' }
}

function readMoved(data: PageHistoryEntry['data']): EntryReading {
  const fromParentId = readParentId(data.from)
  const toParentId = readParentId(data.to)
  if (fromParentId === undefined || toParentId === undefined) {
    console.debug('A page history entry holds a move of an unknown shape', { data })
    return { sentence: 'changed' }
  }
  return {
    sentence: fromParentId === toParentId
      ? 'reordered'
      : 'moved',
    move: { fromParentId, toParentId },
  }
}

function readLabel(data: PageHistoryEntry['data'], sentence: 'labeled' | 'unlabeled'): EntryReading {
  if (typeof data.label !== 'string') {
    console.debug('A page history entry names no label', { data })
    return { sentence: 'changed' }
  }
  return { sentence, label: data.label }
}

function readAttachment(data: PageHistoryEntry['data'], sentence: 'attached' | 'detached'): EntryReading {
  if (typeof data.attachmentId !== 'string' || typeof data.name !== 'string') {
    console.debug('A page history entry names no attachment', { data })
    return { sentence: 'changed' }
  }
  return { sentence, attachment: { id: data.attachmentId, name: data.name }}
}

function readProject(
  data: PageHistoryEntry['data'],
  sentence: 'projectLinked' | 'projectUnlinked',
): EntryReading {
  if (typeof data.projectName !== 'string') {
    console.debug('A page history entry names no project', { data })
    return { sentence: 'changed' }
  }
  return { sentence, project: data.projectName }
}

// `data` is whatever the API stored, in any version of it, so each kind is read defensively: entries
// that came from learnings and the company doc hold less than a page's own.
const readersByKind = {
  created: () => ({ sentence: 'created' }),
  published: readPublished,
  moved: readMoved,
  deleted: (data) => ({
    sentence: data.deletedWith
      ? 'deletedWith'
      : 'deleted',
  }),
  restored: (data) => ({
    sentence: data.restoredWith
      ? 'broughtBackWith'
      : 'broughtBack',
  }),
  labeled: (data) => readLabel(data, 'labeled'),
  unlabeled: (data) => readLabel(data, 'unlabeled'),
  attached: (data) => readAttachment(data, 'attached'),
  detached: (data) => readAttachment(data, 'detached'),
  project_linked: (data) => readProject(data, 'projectLinked'),
  project_unlinked: (data) => readProject(data, 'projectUnlinked'),
} as const satisfies Record<
  string,
  (data: PageHistoryEntry['data'], versionByEntryId: Map<string, number>) => EntryReading
>

function isKnownKind(kind: string): kind is keyof typeof readersByKind {
  return kind in readersByKind
}

// A page's history as a reader takes it, newest first: every version, and everything else that
// happened between them. The API answers oldest first, the order an undo can be matched to the
// entry it reversed in. An entry of a kind this app does not know still reads as a change.
export function describePageHistory(history: PageHistoryEntry[]): PageHistoryRow[] {
  const versionByEntryId = new Map<string, number>()
  const rows: PageHistoryRow[] = []

  for (const entry of history) {
    const version = readNumber(entry.data.version)
    if (version !== null) {
      versionByEntryId.set(entry.id, version)
    }
    const reading: EntryReading = isKnownKind(entry.kind)
      ? readersByKind[entry.kind](entry.data, versionByEntryId)
      : { sentence: 'changed' }
    rows.push({
      entry,
      version,
      otherVersion: null,
      move: null,
      label: null,
      attachment: null,
      project: null,
      ...reading,
    })
  }

  return rows.reverse()
}

// What a page's History tab shows, as its address names it.
export type PageHistoryView =
  | { kind: 'list' }
  | { kind: 'version', version: number }
  // `from` is always the older of the two, whichever way round the address has them.
  | { kind: 'compare', from: number, to: number }

function readVersionParam(value: string | null) {
  const version = Number(value)
  if (Number.isInteger(version) && version > 0) {
    return version
  }
  return null
}

// An address that names nothing readable, as a mistyped one can, shows the list.
export function readPageHistoryView(params: URLSearchParams): PageHistoryView {
  const first = readVersionParam(params.get(PAGE_COMPARE_FROM_PARAM))
  const second = readVersionParam(params.get(PAGE_COMPARE_TO_PARAM))
  const version = readVersionParam(params.get(PAGE_VERSION_PARAM))

  if (first !== null && second !== null && first !== second) {
    return { kind: 'compare', from: Math.min(first, second), to: Math.max(first, second) }
  }
  if (first !== null && first === second) {
    return { kind: 'version', version: first }
  }
  if (version !== null) {
    return { kind: 'version', version }
  }
  return { kind: 'list' }
}

// What an agent is called: what it called itself, or else what it is, since a session and a Project
// Manager name nothing of their own.
export function pageAgentName(agent: LearningAgent, t: TFunction<'knowledge'>) {
  if (agent.name) {
    return agent.name
  }
  const session = /^session:(\d+)$/.exec(agent.id)
  if (session) {
    return t('pages.agentSession', { number: session[1] })
  }
  if (agent.id.startsWith('project_manager:')) {
    return t('pages.agentProjectManager')
  }
  return t('pages.agentUnnamed')
}
