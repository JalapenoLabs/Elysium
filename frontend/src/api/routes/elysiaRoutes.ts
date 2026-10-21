// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// Mirrors `Light` in api/src/elysia/status.rs: the color Elysia shows.
export const ELYSIA_LIGHTS = [ 'running', 'errored', 'needs-you', 'held', 'done' ] as const
export type ElysiaLight = typeof ELYSIA_LIGHTS[number]

// Mirrors `Hold` in api/src/elysia/status.rs: why queued work is not starting.
export type ElysiaHold =
  | { kind: 'paused' }
  | { kind: 'usage-limit', until: string }
  | { kind: 'unavailable' }

// Mirrors the enums in api/src/models/elysia.rs.
export type ElysiaTurnStatus = 'queued' | 'running' | 'succeeded' | 'failed' | 'cancelled'
export type ElysiaQuestionStatus = 'open' | 'answered' | 'dismissed'
export type ElysiaWorkStatus = 'working' | 'ready' | 'blocked' | 'gave-up' | 'done'

// A weekly range of UTC minutes; Monday 00:00 UTC is minute 0.
export type AvailabilityWindow = {
  startMinute: number
  endMinute: number
}

export type ElysiaTurn = {
  id: string
  // `message`, `answer`, `event`, or a kind a later version adds.
  triggerKind: string
  triggerDetail: Record<string, unknown>
  status: ElysiaTurnStatus
  summary: string | null
  error: string | null
  createdBy: string
  createdAt: string
  startedAt: string | null
  finishedAt: string | null
}

export type ElysiaQuestion = {
  id: string
  prompt: string
  options: string[]
  status: ElysiaQuestionStatus
  answer: string | null
  actionItemId: string | null
  createdAt: string
  answeredBy: string | null
  answeredAt: string | null
}

export type ElysiaWork = {
  id: string
  title: string
  detail: string
  status: ElysiaWorkStatus
  readyAt: string | null
  actionItemId: string | null
  revisits: number
  createdAt: string
  updatedAt: string
}

// Elysia at a glance. Replaced whole by every `elysia.updated` event.
export type Elysia = {
  light: ElysiaLight
  hold: ElysiaHold | null
  paused: boolean
  availability: AvailabilityWindow[]
  usagePausedUntil: string | null
  activeTurns: ElysiaTurn[]
  openQuestions: ElysiaQuestion[]
  work: ElysiaWork[]
  updatedBy: string
  updatedAt: string
}

export type ElysiaMessage = {
  id: string
  body: string
  fromElysia: boolean
  createdBy: string
  createdAt: string
}

type GetElysiaResponse = {
  elysia: Elysia
}

export function getElysia() {
  return apiClient
    .get('v1/elysia')
    .json<GetElysiaResponse>()
}

type ListElysiaMessagesResponse = {
  messages: ElysiaMessage[]
}

// Newest first: the page of messages sent before `before`, or the latest ones without it.
export function listElysiaMessages(before?: string) {
  const searchParams = before
    ? { before }
    : undefined
  return apiClient
    .get('v1/elysia/messages', { searchParams })
    .json<ListElysiaMessagesResponse>()
}

type SendElysiaMessageResponse = {
  message: ElysiaMessage
}

export function sendElysiaMessage(body: string) {
  return apiClient
    .post('v1/elysia/messages', { json: { body }})
    .json<SendElysiaMessageResponse>()
}

type ListElysiaTurnsResponse = {
  turns: ElysiaTurn[]
}

export function listElysiaTurns() {
  return apiClient
    .get('v1/elysia/turns')
    .json<ListElysiaTurnsResponse>()
}

type ElysiaTurnResponse = {
  turn: ElysiaTurn
}

export function cancelElysiaTurn(turnId: string) {
  return apiClient
    .post(`v1/elysia/turns/${turnId}/cancel`)
    .json<ElysiaTurnResponse>()
}

type ElysiaQuestionResponse = {
  question: ElysiaQuestion
}

export function answerElysiaQuestion(questionId: string, answer: string) {
  return apiClient
    .post(`v1/elysia/questions/${questionId}/answer`, { json: { answer }})
    .json<ElysiaQuestionResponse>()
}

export function dismissElysiaQuestion(questionId: string) {
  return apiClient
    .post(`v1/elysia/questions/${questionId}/dismiss`)
    .json<ElysiaQuestionResponse>()
}

type UpdateElysiaSettingsRequest = {
  paused?: boolean
  availability?: AvailabilityWindow[]
  // Lifts a usage-limit hold now.
  resumeUsage?: boolean
}

export function updateElysiaSettings(body: UpdateElysiaSettingsRequest) {
  return apiClient
    .patch('v1/elysia/settings', { json: body })
    .json<GetElysiaResponse>()
}
