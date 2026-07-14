// Copyright © 2026 Jalapeno Labs

import type { EventEnvelope } from '../proto/generated/realtime_pb'
import type { ContextSnapshot, RealtimeFrame, RealtimeKind, WindowMessage, WorkerMessage } from './kinds'

// Lib
import { fromBinary } from '@bufbuild/protobuf'

// Redux
import { store } from '../store'
import { realtimeSequenceApplied } from '../store/sessionContextSlice'

// Misc
import { EventEnvelopeSchema } from '../proto/generated/realtime_pb'
import { SessionContextSchema } from '../proto/generated/session_pb'
import { REALTIME_KINDS } from './kinds'
import { seedFromContext } from './seedContext'

// The window side of realtime: subscribe by kind and receive decoded
// `EventEnvelope`s. The transport is the shared worker's single app-wide
// connection; a window that can't reach a SharedWorker degrades to its own
// `EventSource` so realtime never hard-fails (docs/realtime.md).
//
// Reconciliation happens here, centrally: an inbound envelope applies only when
// its sequence is above the store's high-water mark (seeded by `/context`), so
// events already reflected in the bootstrap can never double-apply, and the
// connect-then-fetch race is closed by construction.

type RealtimeHandler = (envelope: EventEnvelope) => void

const handlersByKind = new Map<RealtimeKind, Set<RealtimeHandler>>()

// 'idle' until started; 'worker' is the shared transport; 'direct' the fallback.
let transport: 'idle' | 'worker' | 'direct' = 'idle'
let workerPort: MessagePort | null = null
let directSource: EventSource | null = null

// Stream health, fed by the worker (or the direct stream) for refocus logic.
let isConnected = false
let lastDisconnectedAt: number | null = null

function decodeBase64(data: string): Uint8Array {
  return Uint8Array.from(atob(data), (character) => character.charCodeAt(0))
}

function dispatchFrame(frame: RealtimeFrame) {
  const sequence = Number(frame.id)
  const highWaterMark = store.getState().sessionContext.asOfSequence
  // Already reflected by the /context bootstrap (or an earlier delta): drop it.
  if (sequence && sequence <= highWaterMark) {
    return
  }

  const handlers = handlersByKind.get(frame.kind)
  if (handlers?.size) {
    let envelope: EventEnvelope | null = null
    try {
      envelope = fromBinary(EventEnvelopeSchema, decodeBase64(frame.data))
    }
    catch (error) {
      console.debug('failed to decode a realtime envelope', { kind: frame.kind, error })
    }
    if (envelope) {
      for (const handler of handlers) {
        handler(envelope)
      }
    }
  }

  if (sequence) {
    store.dispatch(realtimeSequenceApplied(sequence))
  }
}

function applySnapshot(snapshot: ContextSnapshot) {
  try {
    const context = fromBinary(SessionContextSchema, decodeBase64(snapshot.data))
    seedFromContext(context)
  }
  catch (error) {
    console.debug('failed to decode a held session snapshot', { error })
  }
}

function onWorkerMessage(message: WorkerMessage) {
  if (message.type === 'frame') {
    dispatchFrame(message.frame)
    return
  }
  if (message.type === 'snapshot') {
    applySnapshot(message.snapshot)
    return
  }
  isConnected = message.connected
  lastDisconnectedAt = message.lastDisconnectedAt
}

function sendToWorker(message: WindowMessage) {
  workerPort?.postMessage(message)
}

function openDirectStream() {
  if (directSource) {
    return
  }
  directSource = new EventSource('/api/v1/events', { withCredentials: true })
  directSource.onopen = () => {
    isConnected = true
  }
  directSource.onerror = () => {
    if (isConnected) {
      lastDisconnectedAt = Date.now()
    }
    isConnected = false
  }
  for (const kind of REALTIME_KINDS) {
    directSource.addEventListener(kind, (event) => {
      dispatchFrame({ kind, id: event.lastEventId, data: event.data })
    })
  }
  transport = 'direct'
}

// Brings the transport up (idempotent). Called before the `/context` read so
// no event can slip between the bootstrap and the stream.
export function startRealtime() {
  if (transport === 'worker') {
    sendToWorker({ type: 'start' })
    return
  }
  if (transport === 'direct') {
    openDirectStream()
    return
  }

  if (typeof SharedWorker !== 'undefined') {
    try {
      const worker = new SharedWorker(
        new URL('./worker.ts', import.meta.url),
        { type: 'module', name: 'elysium-realtime' },
      )
      worker.port.onmessage = (event: MessageEvent<WorkerMessage>) => {
        onWorkerMessage(event.data)
      }
      worker.port.start()
      workerPort = worker.port
      transport = 'worker'
      sendToWorker({ type: 'start' })
      return
    }
    catch (error) {
      console.debug('SharedWorker unavailable; realtime degrades to a direct stream', { error })
    }
  }

  // Degraded fallback: this window owns its own connection (docs/realtime.md).
  openDirectStream()
}

// Tears the stream down when the session ends (an anonymous browser should not
// hammer an endpoint that will only 401).
export function stopRealtime() {
  if (transport === 'worker') {
    sendToWorker({ type: 'stop' })
    return
  }
  directSource?.close()
  directSource = null
  isConnected = false
  transport = 'idle'
}

// Publishes the freshly fetched `/context` to the worker: it becomes the held
// snapshot for future windows and is rebroadcast so sibling windows reseed.
export function shareContextSnapshot(snapshot: ContextSnapshot) {
  if (transport === 'worker') {
    sendToWorker({ type: 'snapshot', snapshot })
  }
}

// Stream health for the refocus heuristic: refetch only when the stream
// actually dropped or the tab slept too long.
export function getStreamHealth() {
  return { isConnected, lastDisconnectedAt } as const
}

// Subscribes to one event kind; returns the unsubscribe.
export function subscribeRealtime(kind: RealtimeKind, handler: RealtimeHandler) {
  let handlers = handlersByKind.get(kind)
  if (!handlers) {
    handlers = new Set()
    handlersByKind.set(kind, handlers)
  }
  handlers.add(handler)

  return () => {
    handlers.delete(handler)
  }
}
