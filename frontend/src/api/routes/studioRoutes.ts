// Copyright © 2026 Jalapeno Labs

import type { CodingSession, TurnState } from './codingSessionRoutes'

// Misc
import { API_BASE_PATH, STUDIO_TURN_UPLOAD_TIMEOUT_MS } from '../../constants'
import { apiClient } from '../index'

// Mirrors the views in api/src/routes/v1/studio_items/mod.rs. Timestamps are UTC ISO 8601 strings.

export type StudioItem = {
  id: string
  title: string
  // The first prompt, kept for the brief a continued thread may start with.
  prompt: string
  projectId: string | null
  // Where every file of the item is kept.
  storageLocationId: string
  // What the tile shows: the pinned image, or the default the API picks.
  thumbnailAssetId: string | null
  // The pinned image; null while the default is shown.
  pinnedAssetId: string | null
  // Distinct image paths and distinct models, however many versions each has.
  imageCount: number
  modelCount: number
  // Why the latest file an agent delivered could not be kept, such as the location's limit.
  // Cleared once a later file is kept.
  pullError: string | null
  deletedAt: string | null
  // Who created it.
  createdBy: string
  createdAt: string
  updatedAt: string
}

export type StudioAssetKind = 'image' | 'model' | 'file'

export type StudioAsset = {
  id: string
  studioItemId: string
  sessionId: number | null
  kind: StudioAssetKind
  // Where the agent wrote it, relative to its `artifacts/` directory: `renders/banana-hero.png`.
  artifactPath: string
  // The file's name, for saving it.
  name: string
  contentType: string | null
  sizeBytes: number
  sha256: string
  createdAt: string
}

// A prompt sent with a drawing.
export type StudioFeedback = {
  id: string
  studioItemId: string
  sessionId: number | null
  // The turn the prompt started, matching its conversation events' `turnId`.
  turnId: string | null
  prompt: string
  // The image, or the model's `.glb`, the drawing was made over.
  sourceAssetId: string | null
  // model-viewer's camera orbit when a model's view was frozen for drawing.
  cameraOrbit: string | null
  // Whether the clean view was kept beside the drawing.
  hasCapture: boolean
  // Who sent it.
  createdBy: string
  createdAt: string
}

type ListStudioItemsResponse = {
  items: StudioItem[]
  // The sessions of the listed items.
  sessions: CodingSession[]
}

type ListStudioItemsOptions = {
  // Softly deleted items instead of live ones.
  deleted?: boolean
}

export function listStudioItems(options: ListStudioItemsOptions = {}) {
  return apiClient
    .get('v1/studio-items', { searchParams: { deleted: Boolean(options.deleted) }})
    .json<ListStudioItemsResponse>()
}

type GetStudioItemResponse = {
  item: StudioItem
  // Newest first.
  assets: StudioAsset[]
  // Oldest first.
  feedback: StudioFeedback[]
  // Oldest first, the order the item's conversation reads in.
  sessions: CodingSession[]
}

export function getStudioItem(itemId: string) {
  return apiClient
    .get(`v1/studio-items/${itemId}`)
    .json<GetStudioItemResponse>()
}

type CreateStudioItemRequest = {
  prompt: string
  // Absent titles the item after the start of the prompt.
  title?: string
  projectId?: string
  storageLocationId: string
  satelliteId: string
}

type CreateStudioItemResponse = {
  item: StudioItem
  // The item's first session, with the prompt queued as its first turn.
  session: CodingSession
}

export function createStudioItem(body: CreateStudioItemRequest) {
  return apiClient
    .post('v1/studio-items', { json: body })
    .json<CreateStudioItemResponse>()
}

// Omitted fields are left unchanged.
type UpdateStudioItemRequest = {
  title?: string
  // Pins an image as the tile's thumbnail; null unpins it, showing the default again.
  thumbnailAssetId?: string | null
}

type StudioItemResponse = {
  item: StudioItem
}

export function updateStudioItem(itemId: string, body: UpdateStudioItemRequest) {
  return apiClient
    .patch(`v1/studio-items/${itemId}`, { json: body })
    .json<StudioItemResponse>()
}

// Soft by default: the item can be restored. `permanently` also removes its files,
// conversation, and agent memory, for good.
export function deleteStudioItem(itemId: string, options: { permanently: boolean }) {
  return apiClient
    .delete(`v1/studio-items/${itemId}`, { searchParams: { permanently: options.permanently }})
}

export function restoreStudioItem(itemId: string) {
  return apiClient
    .post(`v1/studio-items/${itemId}/restore`)
    .json<StudioItemResponse>()
}

export type SendStudioTurnRequest = {
  prompt: string
  // The satellite to continue on when the item's latest thread has ended. Absent lets the
  // API pick the previous one, and it answers 409 when it knows of none usable.
  satelliteId?: string
  // The image, or the model's `.glb`, a drawing was made over.
  sourceAssetId?: string
  // model-viewer's `getCameraOrbit().toString()` when the view drawn over was a model's.
  cameraOrbit?: string
  // The drawing flattened over the view, as PNG.
  annotated?: Blob
  // The clean view without the drawing, as PNG.
  capture?: Blob
}

type SendStudioTurnResponse = {
  // The session the turn runs in: a new one when the latest thread had ended.
  session: CodingSession
  turn: {
    turnId: string
    status: TurnState
    prompt: string
    queuedAt: string | null
  }
  // Recorded only for a prompt with a drawing.
  feedback: StudioFeedback | null
  // The turn opened a new session on the item.
  continued: boolean
}

// Always multipart, so a drawn prompt and a plain one travel the same way. The browser
// sets the multipart boundary itself, so no Content-Type is given.
export function sendStudioTurn(itemId: string, request: SendStudioTurnRequest) {
  const body = new FormData()
  body.set('prompt', request.prompt)
  if (request.satelliteId) {
    body.set('satelliteId', request.satelliteId)
  }
  if (request.sourceAssetId) {
    body.set('sourceAssetId', request.sourceAssetId)
  }
  if (request.cameraOrbit) {
    body.set('cameraOrbit', request.cameraOrbit)
  }
  if (request.annotated) {
    body.set('annotated', request.annotated, 'annotated.png')
  }
  if (request.capture) {
    body.set('capture', request.capture, 'capture.png')
  }

  return apiClient
    .post(`v1/studio-items/${itemId}/turns`, { body, timeout: STUDIO_TURN_UPLOAD_TIMEOUT_MS })
    .json<SendStudioTurnResponse>()
}

// File URLs, for `<img>`, the 3D viewer, and download links. The API streams them from the
// item's storage location, so no provider URL or credential reaches the browser.

export function getStudioAssetContentUrl(itemId: string, assetId: string, options: { download?: boolean } = {}) {
  const url = `${API_BASE_PATH}/v1/studio-items/${itemId}/assets/${assetId}/content`
  if (!options.download) {
    return url
  }
  return `${url}?download=true`
}

export type StudioFeedbackImage = 'annotated' | 'capture'

export function getStudioFeedbackImageUrl(itemId: string, feedbackId: string, image: StudioFeedbackImage) {
  return `${API_BASE_PATH}/v1/studio-items/${itemId}/feedback/${feedbackId}/${image}`
}
