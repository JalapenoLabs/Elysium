// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type {
  LinkKind,
  LinkProvider,
  LinkState,
  LinkTarget,
  PendingWrite,
} from '../../api/routes/actionItemRoutes'
import type { GithubCredential, GithubRepository } from '../../api/routes/githubRoutes'
import type { JiraCredential } from '../../api/routes/jiraRoutes'
import type { PickerOption } from '../../components/MultiPicker'

type ChipColor = 'accent' | 'success' | 'warning' | 'danger' | 'default'

// Names and colors for links, containers, and the writes links owe. Pure, so the item page,
// the initiative page, and the pickers share one spelling of each.

export const providerLabelKeys = {
  jira: 'links.providers.jira',
  github: 'links.providers.github',
} as const satisfies Record<LinkProvider, ParseKeys<'actionItems'>>

export const linkKindLabelKeys = {
  'issue': 'links.kinds.issue',
  'pull-request': 'links.kinds.pull-request',
} as const satisfies Record<LinkKind, ParseKeys<'actionItems'>>

export const linkStateLabelKeys = {
  'open': 'links.states.open',
  'done': 'links.states.done',
  'not-planned': 'links.states.not-planned',
  'merged': 'links.states.merged',
  'closed-unmerged': 'links.states.closed-unmerged',
} as const satisfies Record<LinkState, ParseKeys<'actionItems'>>

export const linkStateChipColors = {
  'open': 'default',
  'done': 'success',
  'not-planned': 'default',
  'merged': 'success',
  'closed-unmerged': 'warning',
} as const satisfies Record<LinkState, ChipColor>

// A credential in a picker is its provider and its id in one key, since the Jira and GitHub
// lists share one select.
const CREDENTIAL_KEY_SEPARATOR = ':'

export function toCredentialKey(provider: LinkProvider, credentialId: string) {
  return `${provider}${CREDENTIAL_KEY_SEPARATOR}${credentialId}`
}

export function fromCredentialKey(key: string): { provider: LinkProvider, credentialId: string } | null {
  // Empty is a picker with no credential chosen yet, not a broken key.
  if (!key) {
    return null
  }
  const [ provider, credentialId ] = key.split(CREDENTIAL_KEY_SEPARATOR)
  if (!credentialId || (provider !== 'jira' && provider !== 'github')) {
    console.debug('A credential key names no provider and credential', { key })
    return null
  }
  return { provider, credentialId }
}

// Every Jira site and GitHub token as one list of options, each labelled with its provider.
export function credentialOptions(
  jiraCredentials: Pick<JiraCredential, 'id' | 'name'>[],
  githubCredentials: Pick<GithubCredential, 'id' | 'name'>[],
  providerLabels: Record<LinkProvider, string>,
): PickerOption[] {
  const options: PickerOption[] = []
  for (const credential of jiraCredentials) {
    options.push({
      id: toCredentialKey('jira', credential.id),
      label: `${providerLabels.jira}: ${credential.name}`,
    })
  }
  for (const credential of githubCredentials) {
    options.push({
      id: toCredentialKey('github', credential.id),
      label: `${providerLabels.github}: ${credential.name}`,
    })
  }
  return options
}

// What an empty search lists: issues touched lately. Jira Cloud refuses a search with no
// condition at all, and the recent ones are what a person usually means.
const RECENT_ISSUES_CONDITION = 'updated >= -90d'

// JQL finding issues by what someone typed, newest first. A key such as ELY-12 finds that
// issue alone, whatever `onlyEpics` says, since a person typing a key means it. Anything else
// matches the words in the text, with quotes and backslashes escaped so the text can never
// close the string it is placed in. The server bounds the result to the credential's
// allowlist.
export function buildIssueSearchJql(text: string, onlyEpics = false) {
  const trimmed = text.trim()
  if (/^[A-Za-z][A-Za-z0-9_]+-\d+$/.test(trimmed)) {
    return `key = "${trimmed.toUpperCase()}"`
  }

  const conditions: string[] = []
  if (trimmed) {
    const escaped = trimmed
      .replaceAll('\\', '\\\\')
      .replaceAll('"', '\\"')
    conditions.push(`text ~ "${escaped}"`)
  }
  if (onlyEpics) {
    conditions.push('issuetype = Epic')
  }
  if (!conditions.length) {
    conditions.push(RECENT_ISSUES_CONDITION)
  }
  return `${conditions.join(' AND ')} ORDER BY updated DESC`
}

// What a pending write shows: what it does, and how its tries have gone.
export type PendingWriteDescription = {
  labelKey: ParseKeys<'actionItems'>
  // Null until the watcher first tries it, when it is simply on its way.
  attempts: number | null
  lastError: string | null
}

const writeLabelKeys = {
  close: 'links.pending.close',
  comment: 'links.pending.comment',
} as const satisfies Record<PendingWrite['kind'], ParseKeys<'actionItems'>>

export function describePendingWrite(write: PendingWrite): PendingWriteDescription {
  return {
    labelKey: writeLabelKeys[write.kind],
    attempts: write.attempts > 0
      ? write.attempts
      : null,
    lastError: write.lastError,
  }
}

// What a link picker holds while a person chooses. The repository matters to GitHub alone.
export type LinkTargetDraft = {
  // A key from `toCredentialKey`, or empty until one is chosen.
  credentialKey: string
  kind: LinkKind
  repository: GithubRepository | null
  // An issue's key, or owner/name#12; null until one is picked.
  reference: string | null
}

export const EMPTY_LINK_TARGET_DRAFT: LinkTargetDraft = {
  credentialKey: '',
  kind: 'issue',
  repository: null,
  reference: null,
}

// The link target a draft names, or null while it names none. Jira links issues only,
// whatever kind the draft last held for GitHub.
export function toLinkTarget(draft: LinkTargetDraft): LinkTarget | null {
  if (!draft.reference) {
    return null
  }
  const credential = fromCredentialKey(draft.credentialKey)
  if (!credential) {
    return null
  }
  return {
    provider: credential.provider,
    credentialId: credential.credentialId,
    kind: credential.provider === 'jira'
      ? 'issue'
      : draft.kind,
    reference: draft.reference,
  }
}
