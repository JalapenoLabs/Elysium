// Copyright © 2026 Jalapeno Labs

import type { ContextSnapshot, RealtimeFrame, WindowMessage, WorkerMessage } from './kinds'

// Misc
import { REALTIME_KINDS } from './kinds'

// The app's single realtime connection (docs/realtime.md): every window
// connects to this SharedWorker over a MessagePort, and the worker owns ONE
// `EventSource` to the API, fanning each frame to every window. Payloads stay
// base64-encoded here; windows decode only the kinds they subscribe to. The
// session cookie rides along (`withCredentials`), and `EventSource` handles
// reconnect + `Last-Event-ID` resumption natively.
//
// The worker also holds the latest `/context` snapshot (posted up by whichever
// window fetched it) plus the frames that arrived since, so a newly opened
// window restores instantly and replays forward while its own fresh fetch is
// in flight. Correctness never depends on the buffer: every window fetches
// `/context` on load and reconciles by sequence.

// Frames kept past the held snapshot. A tab replaying more than this from
// memory should trust its own /context fetch instead, so overflow just trims.
const FRAME_BUFFER_LIMIT = 500

const ports: MessagePort[] = []
let eventSource: EventSource | null = null

let heldSnapshot: ContextSnapshot | null = null
let frameBuffer: RealtimeFrame[] = []

let isConnected = false
let lastDisconnectedAt: number | null = null

function send(port: MessagePort, message: WorkerMessage) {
  try {
    port.postMessage(message)
  }
  catch (error) {
    console.debug('pruning a dead realtime port', { error })
    const index = ports.indexOf(port)
    if (index >= 0) {
      ports.splice(index, 1)
    }
  }
}

function broadcast(message: WorkerMessage, except?: MessagePort) {
  // Iterate a copy: dead ports get pruned mid-loop.
  for (const port of [ ...ports ]) {
    if (port !== except) {
      send(port, message)
    }
  }
}

function statusMessage(): WorkerMessage {
  return { type: 'status', connected: isConnected, lastDisconnectedAt }
}

function bufferFrame(frame: RealtimeFrame) {
  frameBuffer.push(frame)
  if (frameBuffer.length > FRAME_BUFFER_LIMIT) {
    frameBuffer.shift()
  }
}

function ensureStream() {
  if (eventSource) {
    return
  }
  eventSource = new EventSource('/api/v1/events', { withCredentials: true })

  eventSource.onopen = () => {
    isConnected = true
    broadcast(statusMessage())
  }
  eventSource.onerror = () => {
    // Native EventSource retries on its own; track the drop for refocus logic.
    if (isConnected) {
      lastDisconnectedAt = Date.now()
    }
    isConnected = false
    broadcast(statusMessage())
  }

  for (const kind of REALTIME_KINDS) {
    eventSource.addEventListener(kind, (event) => {
      const frame: RealtimeFrame = { kind, id: event.lastEventId, data: event.data }
      bufferFrame(frame)
      broadcast({ type: 'frame', frame })
    })
  }
}

function closeStream() {
  eventSource?.close()
  eventSource = null
  isConnected = false
  heldSnapshot = null
  frameBuffer = []
}

function holdSnapshot(snapshot: ContextSnapshot, source: MessagePort) {
  heldSnapshot = snapshot
  // Frames the snapshot already covers are no longer worth replaying.
  frameBuffer = frameBuffer.filter((frame) => Number(frame.id) > snapshot.asOfSequence)
  // Other windows reconcile onto the fresh snapshot too (the poster already has it).
  broadcast({ type: 'snapshot', snapshot }, source)
}

function onWindowMessage(port: MessagePort, message: WindowMessage) {
  if (message.type === 'start') {
    ensureStream()
    return
  }
  if (message.type === 'stop') {
    // The session ended; every window's next resolve will start a fresh stream.
    closeStream()
    broadcast(statusMessage())
    return
  }
  holdSnapshot(message.snapshot, port)
}

// `SharedWorkerGlobalScope` isn't in the DOM lib this app compiles against, so
// the connect hook is attached through a narrow structural cast. This is the
// one unavoidable interop point of the worker.
const workerScope = self as unknown as {
  onconnect: ((event: MessageEvent) => void) | null
}

workerScope.onconnect = (event) => {
  const port = event.ports[0]
  ports.push(port)

  port.onmessage = (portEvent: MessageEvent<WindowMessage>) => {
    onWindowMessage(port, portEvent.data)
  }
  port.start()

  // Catch the newcomer up: stream health, the held snapshot, then the frames
  // that arrived after it, then live traffic like everyone else.
  send(port, statusMessage())
  if (heldSnapshot) {
    send(port, { type: 'snapshot', snapshot: heldSnapshot })
    for (const frame of frameBuffer) {
      send(port, { type: 'frame', frame })
    }
  }

  ensureStream()
}
