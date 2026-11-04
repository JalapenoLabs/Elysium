// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { ReplyPlace } from '../api/routes/draftRoutes'
import type { MailAccount } from '../api/routes/mailRoutes'

// Misc
import { REPLY_DRAFT_STORAGE_PREFIX } from '../constants'

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

// What the person typed into a record's reply box, kept for the tab so a reload or a crash
// never loses it. Storage can be unavailable (private windows, old browsers); then nothing is
// kept and the box still works.
export function readSavedReply(recordId: string): string | null {
  try {
    return window.sessionStorage.getItem(REPLY_DRAFT_STORAGE_PREFIX + recordId)
  }
  catch (error) {
    console.debug('A saved reply could not be read', { error, recordId })
    return null
  }
}

export function saveReply(recordId: string, text: string) {
  try {
    // A box emptied by hand forgets what it held, rather than keeping an empty value that would
    // hide a stored draft's text.
    if (!text.trim()) {
      window.sessionStorage.removeItem(REPLY_DRAFT_STORAGE_PREFIX + recordId)
      return
    }
    window.sessionStorage.setItem(REPLY_DRAFT_STORAGE_PREFIX + recordId, text)
  }
  catch (error) {
    console.debug('A reply could not be saved for this tab', { error, recordId })
  }
}

export function forgetReply(recordId: string) {
  try {
    window.sessionStorage.removeItem(REPLY_DRAFT_STORAGE_PREFIX + recordId)
  }
  catch (error) {
    console.debug('A saved reply could not be forgotten', { error, recordId })
  }
}
