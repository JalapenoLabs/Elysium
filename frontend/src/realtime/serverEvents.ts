// Copyright © 2026 Jalapeno Labs

import type { CodingSession, SessionEvent } from '../api/routes/codingSessionRoutes'
import type { Llm } from '../api/routes/llmRoutes'
import type { Project } from '../api/routes/projectRoutes'
import type { Satellite, SatelliteStatus } from '../api/routes/satelliteRoutes'

// Everything the API's event stream can send. Mirrors `ServerEvent` in
// api/src/realtime.rs: each SSE message's data is one of these as JSON.
export type ServerEvent =
  // First message on every connection; load state after it.
  | { type: 'hello' }
  // Events were missed; refetch everything shown.
  | { type: 'resync' }
  | { type: 'llm.upserted', data: Llm }
  | { type: 'llm.deleted', data: { id: string } }
  | { type: 'project.upserted', data: Project }
  | { type: 'project.deleted', data: { id: string } }
  | { type: 'satellite.upserted', data: Satellite }
  // Also removes the satellite's sessions.
  | { type: 'satellite.deleted', data: { id: string } }
  | { type: 'satellite.status', data: SatelliteStatus }
  | { type: 'session.upserted', data: CodingSession }
  | { type: 'session.deleted', data: { id: string } }
  | { type: 'session.event', data: SessionEvent }
  // Live events for this session may have been missed; refetch its history.
  | { type: 'session.resync', data: { id: string } }

export type ServerEventType = ServerEvent['type']
