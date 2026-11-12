// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { Meeting, MeetingHistoryEntry, MeetingVisibility } from '../../../api/routes/meetingRoutes'
import type { User } from '../../../api/routes/userRoutes'

// Who reads a meeting, as its page shows it (docs/meeting-review.md, Who sees a meeting). The API
// decides who may read and change one; these only say so, and keep a page from offering what the
// API would refuse.

export const visibilityLabelKeys = {
  unchecked: 'visibility.unchecked',
  private: 'visibility.private',
  public: 'visibility.public',
} as const satisfies Record<MeetingVisibility, ParseKeys<'meetings'>>

export const visibilityChipColors = {
  unchecked: 'warning',
  private: 'default',
  public: 'success',
} as const satisfies Record<MeetingVisibility, 'warning' | 'default' | 'success'>

// Each names an icon `MeetingVisibility` draws.
export const visibilityIcons = {
  unchecked: 'lockKeyhole',
  private: 'lock',
  public: 'globe',
} as const satisfies Record<MeetingVisibility, string>

const decisionKeys = {
  opened: 'visibility.decisions.opened',
  keptPrivate: 'visibility.decisions.keptPrivate',
  keptPrivateBecause: 'visibility.decisions.keptPrivateBecause',
  undecided: 'visibility.decisions.undecided',
} as const satisfies Record<string, ParseKeys<'meetings'>>

// What a meeting's correction decided about who reads it, as its page says it, when the newest
// change to who reads it was the correction's; null once a person changed it since, or before it
// was decided. Entries outlive the version that wrote them, so their data is read defensively.
export function correctionDecisionOf(entry: MeetingHistoryEntry | null) {
  if (!entry || entry.data.correction !== true) {
    return null
  }
  if (entry.data.after === 'public') {
    return { key: decisionKeys.opened, why: null }
  }
  if (entry.data.undecided === true) {
    return { key: decisionKeys.undecided, why: null }
  }
  const why = typeof entry.data.why === 'string' && entry.data.why.trim()
    ? entry.data.why.trim()
    : null
  if (why) {
    return { key: decisionKeys.keptPrivateBecause, why }
  }
  return { key: decisionKeys.keptPrivate, why: null }
}

// Whether `user` may read `meeting`, mirroring `readable_by` in api/src/models/meeting.rs: its
// owner, someone it is shared with, and admins always; members while it is public; a guest only
// when it is shared with them.
export function mayRead(meeting: Meeting, user: User) {
  if (user.id === meeting.ownerId || meeting.sharedWith.includes(user.id) || user.role === 'admin') {
    return true
  }
  return meeting.visibility === 'public' && user.role === 'member'
}

// What undoing a change of kind `kind` takes, mirroring `need_to_undo` in
// api/src/meetings/access.rs: what making it took. Every other kind takes an editor's. A Map, since
// kinds arrive as text from every version of the API.
const undoNeeds = new Map<string, 'manage' | 'own'>([
  [ 'merged', 'manage' ],
  [ 'split', 'manage' ],
  [ 'visibility_changed', 'manage' ],
  [ 'shared', 'manage' ],
  [ 'unshared', 'manage' ],
  [ 'hold_changed', 'own' ],
  [ 'offset_changed', 'own' ],
])

// Whether the reader of `meeting`, who is `readerId`, may undo a change of kind `kind`.
export function mayUndo(meeting: Meeting, readerId: string | undefined, kind: string) {
  const need = undoNeeds.get(kind)
  if (need === 'manage') {
    return meeting.canManage
  }
  if (need === 'own') {
    return meeting.ownerId === readerId
  }
  return meeting.canEdit
}
