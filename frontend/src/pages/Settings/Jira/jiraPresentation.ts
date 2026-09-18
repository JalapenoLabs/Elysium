// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { JiraBoard, JiraCredential, JiraProject } from '../../../api/routes/jiraRoutes'

// Misc
import { ALL_JIRA_ITEMS } from '../../../api/routes/jiraRoutes'

// A Jira Cloud site is always a subdomain of atlassian.net. Jira Data Center, which lives
// on a customer's own host, is not supported, so no credential carries another endpoint.
export const JIRA_SITE_URL_PATTERN = /^https:\/\/[a-z0-9-]+\.atlassian\.net$/

// The site as the API stores it. A pasted address usually carries a trailing slash, and
// the host is case-insensitive, so both are settled before anything is compared or sent.
export function normalizeJiraSiteUrl(siteUrl: string) {
  return siteUrl
    .trim()
    .replace(/\/+$/, '')
    .toLowerCase()
}

// A stored token belongs to one site and one Atlassian account, as the API enforces:
// changing either means bringing the token that goes with it. Addresses are compared
// case-insensitively, since Atlassian does not tell two spellings apart.
export function keepsStoredJiraToken(
  stored: JiraCredential | null,
  siteUrl: string,
  accountEmail: string,
) {
  if (!stored) {
    return false
  }

  return stored.siteUrl === normalizeJiraSiteUrl(siteUrl)
    && stored.accountEmail.toLowerCase() === accountEmail.trim().toLowerCase()
}

// How a form holds an allowlist: a switch for everything, and the ids picked one by one
// while it is off. Board ids are numbers over the API, as Jira reports them.
export type JiraScopeSelection = {
  allProjects: boolean
  projectIds: string[]
  allBoards: boolean
  boardIds: number[]
}

// A stored credential as the edit form holds it.
export function toJiraScopeSelection(credential: JiraCredential): JiraScopeSelection {
  return {
    allProjects: credential.projects === ALL_JIRA_ITEMS,
    projectIds: credential.projects === ALL_JIRA_ITEMS
      ? []
      : credential.projects.map((project) => project.id),
    allBoards: credential.boards === ALL_JIRA_ITEMS,
    boardIds: credential.boards === ALL_JIRA_ITEMS
      ? []
      : credential.boards.map((board) => board.id),
  }
}

// How many project keys or board names a table cell names before counting the rest.
const SCOPE_PREVIEW_LIMIT = 3

// What a table cell says about an allowlist: everything, nothing, or the first few names
// with however many follow.
export function summarizeScope(names: string[]) {
  return {
    count: names.length,
    preview: names.slice(0, SCOPE_PREVIEW_LIMIT),
    remaining: Math.max(names.length - SCOPE_PREVIEW_LIMIT, 0),
  } as const
}

// The allowed projects and boards the token no longer reaches, named as they were picked.
// An allowlist of everything can never hold one, since it names nothing in particular.
export function findUnreachableNames(
  credential: JiraCredential,
  reachableProjects: JiraProject[],
  reachableBoards: JiraBoard[],
) {
  const projectIds = new Set(reachableProjects.map((project) => project.id))
  const boardIds = new Set(reachableBoards.map((board) => board.id))
  const names: string[] = []

  if (credential.projects !== ALL_JIRA_ITEMS) {
    for (const project of credential.projects) {
      if (!projectIds.has(project.id)) {
        names.push(project.key)
      }
    }
  }

  if (credential.boards !== ALL_JIRA_ITEMS) {
    for (const board of credential.boards) {
      if (!boardIds.has(board.id)) {
        names.push(board.name)
      }
    }
  }

  return names
}

// One setup step. The text is translated; the URL is not.
export type JiraSetupStep = {
  textKey: ParseKeys<'jira'>
  link?: string
}

// Where an Atlassian API token comes from, in Atlassian's own words.
export const JIRA_SETUP_STEPS = [
  { textKey: 'setup.open', link: 'https://id.atlassian.com/manage-profile/security/api-tokens' },
  { textKey: 'setup.name' },
  { textKey: 'setup.copy' },
  { textKey: 'setup.email' },
  { textKey: 'setup.permissions' },
] as const satisfies readonly JiraSetupStep[]
