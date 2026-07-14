// Copyright © 2026 Jalapeno Labs

// The SSE event kinds the server publishes (docs/realtime.md). The worker
// listens for exactly these; windows subscribe by kind. New server kinds are
// added here and picked up by both sides.
export const REALTIME_KINDS = [
  'org.updated',
  'profile.updated',
  'presence.changed',
  'session.changed',
  'snapshot',
] as const

export type RealtimeKind = typeof REALTIME_KINDS[number]

// One SSE frame as it travels from the worker to a window: the payload stays
// base64 so each window decodes only the kinds it actually subscribes to. `id`
// is the envelope's per-user sequence (the SSE `id:` line).
export type RealtimeFrame = {
  kind: RealtimeKind
  id: string
  data: string
}

// The held `GET /api/v1/context` response (binary protobuf, base64) plus its
// realtime high-water mark. The worker keeps the latest one so a new window
// restores instantly while its own fresh fetch is in flight.
export type ContextSnapshot = {
  data: string
  asOfSequence: number
}

// Worker -> window.
export type WorkerMessage =
  | { type: 'frame', frame: RealtimeFrame }
  | { type: 'snapshot', snapshot: ContextSnapshot }
  | { type: 'status', connected: boolean, lastDisconnectedAt: number | null }

// Window -> worker.
export type WindowMessage =
  | { type: 'start' }
  | { type: 'stop' }
  | { type: 'snapshot', snapshot: ContextSnapshot }
