// Copyright © 2026 Jalapeno Labs

import type { HistoryEntry, LinkProvider } from './actionItemRoutes'

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

// Mirrors `ContainerKind` in api/src/models/initiative_link.rs: an epic or saved filter on
// Jira, a milestone or label on GitHub.
export const CONTAINER_KINDS = [ 'epic', 'filter', 'milestone', 'label' ] as const
export type ContainerKind = typeof CONTAINER_KINDS[number]

// Mirrors `InitiativeLinkResponse` in api/src/routes/v1/initiatives/mod.rs.
export type InitiativeLink = {
  id: string
  initiativeId: string
  provider: LinkProvider
  kind: ContainerKind
  credentialId: string
  // ELY-7 for an epic, a filter's id, owner/name#3, or owner/name:label.
  key: string
  url: string
  // The container's name as last read.
  title: string
  // When the watcher last read every child; null until it first has.
  syncedAt: string | null
  // Why the latest read failed; null when it succeeded.
  syncError: string | null
  // The container held more children than the watcher reads.
  truncated: boolean
  createdAt: string
  updatedAt: string
}

// What a container request names: a container a picker listed.
export type ContainerTarget = {
  provider: LinkProvider
  credentialId: string
  kind: ContainerKind
  // An epic's key, a filter's id, owner/name#3, or owner/name:label.
  reference: string
}

type ListInitiativeLinksResponse = {
  links: InitiativeLink[]
}

// Oldest first.
export function listInitiativeLinks(initiativeId: string) {
  return apiClient
    .get(`v1/initiatives/${initiativeId}/links`)
    .json<ListInitiativeLinksResponse>()
}

type InitiativeLinkResponse = {
  link: InitiativeLink
}

// The container is read through the credential first; its children join on the watcher's
// next pass, which this wakes at once.
export function addInitiativeLink(initiativeId: string, target: ContainerTarget) {
  return apiClient
    .post(`v1/initiatives/${initiativeId}/links`, { json: target })
    .json<InitiativeLinkResponse>()
}

// The items the container brought in leave the initiative; the items themselves stay.
export function removeInitiativeLink(initiativeId: string, linkId: string) {
  return apiClient
    .delete(`v1/initiatives/${initiativeId}/links/${linkId}`)
}
