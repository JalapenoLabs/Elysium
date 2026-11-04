// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { ReplyPlace } from '../api/routes/draftRoutes'
import type { MailAccount } from '../api/routes/mailRoutes'

// What each place a chat reply can go is called.
export const placeLabelKeys = {
  thread: 'reply.places.thread',
  conversation: 'reply.places.conversation',
} as const satisfies Record<ReplyPlace, ParseKeys<'timeline'>>

// The mailboxes `personId` can reply from: their own, turned on. The API sends from a mailbox
// for the person who connected it alone.
export function sendingMailboxes(accounts: MailAccount[], personId: string | null) {
  const own: MailAccount[] = []
  for (const account of accounts) {
    if (account.createdBy === personId && account.isActive) {
      own.push(account)
    }
  }
  return own
}

// A mail draft's recipients, from its target, for showing whom it goes to.
export function mailRecipients(target: Record<string, unknown>) {
  const to = target.to
  if (!Array.isArray(to)) {
    return ''
  }
  return to.filter((recipient) => typeof recipient === 'string').join(', ')
}
