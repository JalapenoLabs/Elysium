// Copyright © 2026 Jalapeno Labs

import type { HistoryEntry } from './actionItemRoutes'

// Misc
import { apiClient } from '../index'

// Mirrors `InitiativeState` in api/src/models/initiative.rs.
export const INITIATIVE_STATES = [ 'active', 'achieved', 'abandoned' ] as const
export type InitiativeState = typeof INITIATIVE_STATES[number]

// Mirrors `Progress` in api/src/action_items/progress.rs: resolved members out of the
// total that count. Dismissed and deleted members count toward neither.
export type InitiativeProgress = {
  resolved: number
  total: number
}

// Mirrors `BurnupPoint` in api/src/action_items/progress.rs: progress as it stood at `at`.
export type BurnupPoint = InitiativeProgress & {
  at: string
}

// Mirrors `InitiativeResponse` in api/src/routes/v1/initiatives/mod.rs.
export type Initiative = {
  id: string
  name: string
  description: string
  state: InitiativeState
  targetAt: string | null
  projectIds: string[]
  // Progress as of the moment the API answered.
  progress: InitiativeProgress
  // Set while the initiative is softly deleted; it can be restored.
  deletedAt: string | null
  createdAt: string
  updatedAt: string
}

type ListInitiativesQuery = {
  // Deleted initiatives only, instead of the rest.
  deleted?: boolean
}

type ListInitiativesResponse = {
  initiatives: Initiative[]
}

// By name.
export function listInitiatives(query: ListInitiativesQuery = {}) {
  const searchParams = query.deleted
    ? { deleted: 'true' }
    : undefined
  return apiClient
    .get('v1/initiatives', { searchParams })
    .json<ListInitiativesResponse>()
}

type InitiativeResponse = {
  initiative: Initiative
}

// Answers deleted initiatives too.
export function getInitiative(initiativeId: string) {
  return apiClient
    .get(`v1/initiatives/${initiativeId}`)
    .json<InitiativeResponse>()
}

type CreateInitiativeRequest = {
  name: string
  description?: string
  targetAt?: string | null
  projectIds?: string[]
}

// It starts `active`, with no items.
export function createInitiative(body: CreateInitiativeRequest) {
  return apiClient
    .post('v1/initiatives', { json: body })
    .json<InitiativeResponse>()
}

// Omitted fields are left unchanged; `targetAt: null` removes the target. The state moves in
// any direction.
type UpdateInitiativeRequest = {
  name?: string
  description?: string
  targetAt?: string | null
  state?: InitiativeState
}

export function updateInitiative(initiativeId: string, body: UpdateInitiativeRequest) {
  return apiClient
    .patch(`v1/initiatives/${initiativeId}`, { json: body })
    .json<InitiativeResponse>()
}

// Soft: the initiative keeps its members and can be restored.
export function deleteInitiative(initiativeId: string) {
  return apiClient
    .delete(`v1/initiatives/${initiativeId}`)
}

export function restoreInitiative(initiativeId: string) {
  return apiClient
    .post(`v1/initiatives/${initiativeId}/restore`)
    .json<InitiativeResponse>()
}

// Progress now, and the burnup: a point at creation, one at every moment either count
// changed, and one now. Clients draw it as steps.
export type InitiativeProgressResponse = InitiativeProgress & {
  burnup: BurnupPoint[]
}

export function getInitiativeProgress(initiativeId: string) {
  return apiClient
    .get(`v1/initiatives/${initiativeId}/progress`)
    .json<InitiativeProgressResponse>()
}

type ListHistoryResponse = {
  history: HistoryEntry[]
}

// Oldest first, including items joining and leaving.
export function listInitiativeHistory(initiativeId: string) {
  return apiClient
    .get(`v1/initiatives/${initiativeId}/history`)
    .json<ListHistoryResponse>()
}

// Idempotent, like the item membership routes.

export function addInitiativeProject(initiativeId: string, projectId: string) {
  return apiClient
    .put(`v1/initiatives/${initiativeId}/projects/${projectId}`)
    .json<InitiativeResponse>()
}

export function removeInitiativeProject(initiativeId: string, projectId: string) {
  return apiClient
    .delete(`v1/initiatives/${initiativeId}/projects/${projectId}`)
    .json<InitiativeResponse>()
}
