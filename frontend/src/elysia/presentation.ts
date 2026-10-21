// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type {
  ElysiaHold,
  ElysiaLight,
  ElysiaTurnStatus,
  ElysiaWorkStatus,
} from '../api/routes/elysiaRoutes'

// How Elysia's statuses look and read, in one place for the toggle, the panel, and her
// activity. Colors are the `--elysia-*` theme tokens in src/theme/matter.css.

export const lightDotClassNames = {
  'running': 'bg-elysia-running animate-pulse',
  'errored': 'bg-elysia-errored',
  'needs-you': 'bg-elysia-needs-you animate-pulse',
  'done': 'bg-elysia-done',
} as const satisfies Record<ElysiaLight, string>

export const lightTextClassNames = {
  'running': 'text-elysia-running',
  'errored': 'text-elysia-errored',
  'needs-you': 'text-elysia-needs-you',
  'done': 'text-elysia-done',
} as const satisfies Record<ElysiaLight, string>

export const lightLabelKeys = {
  'running': 'light.running',
  'errored': 'light.errored',
  'needs-you': 'light.needs-you',
  'done': 'light.done',
} as const satisfies Record<ElysiaLight, ParseKeys<'elysia'>>

export const holdLabelKeys = {
  'paused': 'hold.paused',
  'usage-limit': 'hold.usageLimit',
  'no-runner': 'hold.noRunner',
  'unavailable': 'hold.unavailable',
} as const satisfies Record<ElysiaHold['kind'], ParseKeys<'elysia'>>

export const workStatusLabelKeys = {
  'working': 'work.status.working',
  'ready': 'work.status.ready',
  'blocked': 'work.status.blocked',
  'gave-up': 'work.status.gave-up',
  'done': 'work.status.done',
} as const satisfies Record<ElysiaWorkStatus, ParseKeys<'elysia'>>

export const turnStatusLabelKeys = {
  queued: 'activity.status.queued',
  running: 'activity.status.running',
  succeeded: 'activity.status.succeeded',
  failed: 'activity.status.failed',
  cancelled: 'activity.status.cancelled',
} as const satisfies Record<ElysiaTurnStatus, ParseKeys<'elysia'>>

// Trigger kinds are open-ended on the server, so this is a map rather than a typed record,
// and an unknown kind reads as "Other".
const triggerLabelKeys = new Map<string, ParseKeys<'elysia'>>([
  [ 'message', 'activity.trigger.message' ],
  [ 'answer', 'activity.trigger.answer' ],
  [ 'event', 'activity.trigger.event' ],
  [ 'idle', 'activity.trigger.idle' ],
])

export function triggerLabelKey(triggerKind: string): ParseKeys<'elysia'> {
  return triggerLabelKeys.get(triggerKind) ?? 'activity.trigger.other'
}
