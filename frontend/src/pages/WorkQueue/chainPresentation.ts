// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { IconType } from 'react-icons'
import type { WorkBatchLink, WorkBatchLinkKind } from '../../api/routes/workQueueRoutes'

// User interface
import {
  LuCircleCheckBig,
  LuFileText,
  LuGitPullRequest,
  LuLayers,
  LuMessageSquare,
  LuSearch,
  LuTicket,
} from 'react-icons/lu'

// Misc
import {
  getActionItemViewUrl,
  getKnowledgeDocumentUrl,
  getProjectFindingsUrl,
  getProjectManagerUrl,
  getWorkBatchViewUrl,
} from '../../urls'

// How a batch's chain reads, link by link (docs/autonomy.md, Staying aware): what each kind is
// called, its icon, its text, and where it opens. Pure, and tested beside.

// What each kind of link is called. A record is called by where it came from instead
// (`sourceLabelKey`), which says more.
export const chainLinkLabelKeys = {
  finding: 'batch.why.kinds.finding',
  message: 'batch.why.kinds.message',
  record: 'batch.why.kinds.record',
  issue: 'batch.why.kinds.issue',
  item: 'batch.why.kinds.item',
  batch: 'batch.why.kinds.batch',
  pullRequest: 'batch.why.kinds.pullRequest',
} as const satisfies Record<WorkBatchLinkKind, ParseKeys<'workQueue'>>

export const chainLinkIcons = {
  finding: LuSearch,
  message: LuMessageSquare,
  record: LuFileText,
  issue: LuTicket,
  item: LuCircleCheckBig,
  batch: LuLayers,
  pullRequest: LuGitPullRequest,
} as const satisfies Record<WorkBatchLinkKind, IconType>

// What a link reads as: what was seen, the message, the record's title under its meeting's,
// the issue's key and title, or the pull request's reference.
export function chainLinkText(link: WorkBatchLink): string {
  switch (link.kind) {
    case 'finding':
      return link.title
    case 'message':
      return link.body
    case 'record':
      if (link.meetingTitle) {
        return `${link.meetingTitle}: ${link.title}`
      }
      return link.title
    case 'issue':
      return `${link.key} ${link.title}`
    case 'item':
    case 'batch':
      return link.title
    case 'pullRequest':
      return `${link.repository}#${link.number}`
    default:
      // The chain is the API's answer, data from outside the app, so a kind this build does not
      // know reads as nothing rather than breaking the page.
      console.debug('chainLinkText met a link of a kind it does not know', link)
      return ''
  }
}

// Where a link opens: a record its knowledge base document, which for a meeting segment is the
// meeting at that topic; a message its Project Manager's conversation and a finding the Findings
// list, both on the batch's project, `projectId`; an item and a batch their pages; an issue its
// tracker and a pull request GitHub, both outside Elysium. A record not indexed yet opens nowhere.
export function chainLinkTarget(link: WorkBatchLink, projectId: string) {
  switch (link.kind) {
    case 'finding':
      return { href: getProjectFindingsUrl(projectId), isExternal: false } as const
    case 'message':
      return { href: getProjectManagerUrl(projectId), isExternal: false } as const
    case 'record':
      if (!link.documentId) {
        return null
      }
      return { href: getKnowledgeDocumentUrl(link.documentId), isExternal: false } as const
    case 'issue':
    case 'pullRequest':
      return { href: link.url, isExternal: true } as const
    case 'item':
      return { href: getActionItemViewUrl(link.id), isExternal: false } as const
    case 'batch':
      return { href: getWorkBatchViewUrl(link.id), isExternal: false } as const
    default:
      console.debug('chainLinkTarget met a link of a kind it does not know', link)
      return null
  }
}

// A link's key among its chain's, unique even where two kinds share an id.
export function chainLinkKey(link: WorkBatchLink): string {
  if (link.kind === 'issue') {
    return `issue:${link.actionItemId}:${link.key}`
  }
  return `${link.kind}:${link.id}`
}
