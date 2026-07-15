// Copyright © 2026 Jalapeno Labs

import type { SessionContext } from '../proto/generated/session_pb'

// Redux
import { dispatch } from '../store'
import { sessionAuthenticated } from '../store/sessionSlice'
import { currentUserLoaded, toCurrentUser } from '../store/currentUserSlice'
import { sessionContextSeeded } from '../store/sessionContextSlice'

// Seeds every session-scoped slice from one `SessionContext` (the `/context`
// bootstrap response, or the worker's held snapshot when a new window opens).
// This is the single gate the app boots through (docs/realtime.md).
export function seedFromContext(context: SessionContext) {
  const user = context.user
  if (!user) {
    console.debug('session context arrived without a user; ignoring', { context })
    return
  }

  dispatch(sessionAuthenticated({
    subject: user.subject,
    email: user.email,
    username: user.displayName,
  }))
  dispatch(currentUserLoaded(toCurrentUser(user)))
  dispatch(sessionContextSeeded({
    accounts: context.accounts.map((account) => ({
      userId: account.userId?.value ?? '',
      email: account.email,
      displayName: account.displayName,
      avatarUrl: account.avatarUrl,
      isActive: account.isActive,
    })),
    orgs: context.orgs.map((org) => ({
      id: org.id?.value ?? '',
      name: org.name,
      slug: org.slug,
      role: org.role,
      logoUrl: org.logoUrl,
      accentColor: org.accentColor,
      wordmark: org.wordmark,
    })),
    activeOrgId: context.activeOrgId?.value ?? null,
    totpEnrollmentRequired: context.totpEnrollmentRequired,
    activeOrgSuspended: context.activeOrgSuspended,
    asOfSequence: Number(context.asOfSequence),
  }))
}
