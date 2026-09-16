// Copyright © 2026 Jalapeno Labs

import type { CodingSession, SessionEvent } from '../api/routes/codingSessionRoutes'
import type { EnvironmentVariable } from '../api/routes/environmentRoutes'
import type { GithubCredential } from '../api/routes/githubRoutes'
import type { Llm } from '../api/routes/llmRoutes'
import type { MailAccount, MailDomain, MailServer } from '../api/routes/mailRoutes'
import type { Project } from '../api/routes/projectRoutes'
import type { Satellite, SatelliteStatus } from '../api/routes/satelliteRoutes'
import type { StorageLocation } from '../api/routes/storageRoutes'

// Everything the API's event stream can send. Mirrors `ServerEvent` in
// api/src/realtime.rs: each SSE message's data is one of these as JSON.
export type ServerEvent =
  // First message on every connection; load state after it.
  | { type: 'hello' }
  // Events were missed; refetch everything shown.
  | { type: 'resync' }
  | { type: 'environmentVariable.upserted', data: EnvironmentVariable }
  | { type: 'environmentVariable.deleted', data: { id: string } }
  | { type: 'githubCredential.upserted', data: GithubCredential }
  | { type: 'githubCredential.deleted', data: { id: string } }
  | { type: 'llm.upserted', data: Llm }
  | { type: 'llm.deleted', data: { id: string } }
  | { type: 'mailbox.upserted', data: MailAccount }
  | { type: 'mailbox.deleted', data: { id: string } }
  // Also sent for each step while the server is created.
  | { type: 'mailServer.updated', data: MailServer }
  | { type: 'mailDomain.upserted', data: MailDomain }
  | { type: 'mailDomain.deleted', data: { id: string } }
  | { type: 'project.upserted', data: Project }
  | { type: 'project.deleted', data: { id: string } }
  | { type: 'satellite.upserted', data: Satellite }
  // Also removes the satellite's sessions.
  | { type: 'satellite.deleted', data: { id: string } }
  | { type: 'satellite.status', data: SatelliteStatus }
  | { type: 'storageLocation.upserted', data: StorageLocation }
  | { type: 'storageLocation.deleted', data: { id: string } }
  | { type: 'session.upserted', data: CodingSession }
  | { type: 'session.deleted', data: { id: string } }
  | { type: 'session.event', data: SessionEvent }
  // Live events for this session may have been missed; refetch its history.
  | { type: 'session.resync', data: { id: string } }

export type ServerEventType = ServerEvent['type']
