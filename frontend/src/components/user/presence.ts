// Copyright © 2026 Jalapeno Labs

// Misc
import { ManualStatusKind, Presence } from '../../proto/generated/user_pb'

// The visual grammar for a user's status, shared by every surface that renders
// a presence dot (the navbar status control, the profile badge, and future
// people lists). Presence is derived server-side; a manual override supersedes
// it while active. See docs/identity.md.

export const dotColorByPresence = {
  [Presence.UNSPECIFIED]: 'bg-default-400',
  [Presence.ONLINE]: 'bg-success',
  [Presence.AWAY]: 'bg-warning',
  [Presence.OFFLINE]: 'bg-default-400',
} as const satisfies Record<Presence, string>

export const labelKeyByPresence = {
  [Presence.UNSPECIFIED]: 'status.offline',
  [Presence.ONLINE]: 'status.online',
  [Presence.AWAY]: 'status.away',
  [Presence.OFFLINE]: 'status.offline',
} as const satisfies Record<Presence, string>

export const dotColorByManualStatus = {
  [ManualStatusKind.UNSPECIFIED]: 'bg-default-400',
  [ManualStatusKind.OUT_OF_OFFICE]: 'bg-secondary',
  [ManualStatusKind.DO_NOT_DISTURB]: 'bg-danger',
  [ManualStatusKind.CUSTOM]: 'bg-primary',
} as const satisfies Record<ManualStatusKind, string>

export const labelKeyByManualStatus = {
  [ManualStatusKind.UNSPECIFIED]: 'status.offline',
  [ManualStatusKind.OUT_OF_OFFICE]: 'status.outOfOffice',
  [ManualStatusKind.DO_NOT_DISTURB]: 'status.doNotDisturb',
  [ManualStatusKind.CUSTOM]: 'status.custom',
} as const satisfies Record<ManualStatusKind, string>

type StatusFields = {
  presence: Presence
  manualStatus: ManualStatusKind
  manualStatusText: string
}

// What currently governs the user's visible status: the manual override when
// one is active, the derived presence otherwise. `customText` is set only for
// an active CUSTOM override; callers render it in place of the label.
export function effectiveStatusOf(user: StatusFields) {
  if (user.manualStatus !== ManualStatusKind.UNSPECIFIED) {
    return {
      hasOverride: true,
      dotColor: dotColorByManualStatus[user.manualStatus],
      labelKey: labelKeyByManualStatus[user.manualStatus],
      customText: user.manualStatus === ManualStatusKind.CUSTOM ? user.manualStatusText : '',
    } as const
  }

  return {
    hasOverride: false,
    dotColor: dotColorByPresence[user.presence],
    labelKey: labelKeyByPresence[user.presence],
    customText: '',
  } as const
}
