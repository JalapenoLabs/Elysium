// Copyright © 2026 Jalapeno Labs

import type { ConnectionListeners } from './nekoConnection'

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Misc
import { PASTE_KEYSYMS } from './keysyms'
import { NekoConnection } from './nekoConnection'
import { encodeButton, encodeKey, POINTER_BUTTONS } from './nekoInput'

// The signaling socket, driven by the test: what the connection sends is recorded, and what
// Neko would answer is pushed in through `receive`.
class FakeSocket {
  static latest: FakeSocket | null = null
  readyState: number = WebSocket.OPEN
  sent: { event: string, payload?: unknown }[] = []
  onmessage: ((message: { data: string }) => void) | null = null
  onclose: (() => void) | null = null
  onerror: ((error: unknown) => void) | null = null

  constructor() {
    FakeSocket.latest = this
  }

  send(data: string) {
    this.sent.push(JSON.parse(data))
  }

  close() {
    this.readyState = WebSocket.CLOSED
  }

  receive(event: string, payload?: unknown) {
    this.onmessage?.({ data: JSON.stringify({ event, payload }) })
  }
}

class FakeChannel {
  readyState = 'open'
  binaryType = 'blob'
  sent: ArrayBuffer[] = []
  onmessage: ((message: { data: unknown }) => void) | null = null

  send(buffer: ArrayBuffer) {
    this.sent.push(buffer)
  }
}

class FakePeer {
  static latest: FakePeer | null = null
  remoteDescription: unknown = null
  connectionState = 'new'
  onicecandidate: ((event: unknown) => void) | null = null
  onconnectionstatechange: (() => void) | null = null
  ontrack: ((event: unknown) => void) | null = null
  ondatachannel: ((event: { channel: FakeChannel }) => void) | null = null

  constructor() {
    FakePeer.latest = this
  }

  async setRemoteDescription(description: unknown) {
    this.remoteDescription = description
  }

  async addIceCandidate() {}

  async createAnswer() {
    return { type: 'answer', sdp: 'answer' }
  }

  async setLocalDescription() {}

  close() {
    this.connectionState = 'closed'
  }
}

function bytes(buffer: ArrayBuffer) {
  return [ ...new Uint8Array(buffer) ]
}

function listeners() {
  return {
    onState: vi.fn(),
    onStream: vi.fn(),
    onScreenSize: vi.fn(),
    onCursor: vi.fn(),
  } satisfies ConnectionListeners
}

// Opens a connection through to its data channel, as Neko's offer would.
async function connected(connectionListeners = listeners()) {
  const connection = new NekoConnection('wss://elysium.test/stream', connectionListeners)
  connection.open()
  const socket = FakeSocket.latest!
  socket.receive('system/init', {})
  socket.receive('signal/provide', { sdp: 'offer' })
  await vi.waitFor(() => expect(socket.sent.some((message) => message.event === 'signal/answer')).toBe(true))

  const peer = FakePeer.latest!
  const channel = new FakeChannel()
  peer.ondatachannel?.({ channel })
  return { connection, socket, peer, channel, listeners: connectionListeners }
}

describe('NekoConnection', () => {
  beforeEach(() => {
    vi.stubGlobal('WebSocket', Object.assign(FakeSocket, { OPEN: 1, CLOSED: 3 }))
    vi.stubGlobal('RTCPeerConnection', FakePeer)
  })

  afterEach(() => {
    vi.useRealTimers()
    vi.unstubAllGlobals()
  })

  // Reconnecting closed the old connection, whose socket then reported its own close a
  // moment later and pushed "disconnected" over the new connection's state.
  it('reports nothing once closed, whatever its old socket and peer do afterwards', async () => {
    const { connection, socket, peer, listeners: connectionListeners } = await connected()
    connection.close()
    expect(connectionListeners.onState).toHaveBeenLastCalledWith('disconnected')
    const reported = connectionListeners.onState.mock.calls.length

    socket.onclose?.()
    peer.connectionState = 'failed'
    peer.onconnectionstatechange?.()
    peer.ontrack?.({ track: { kind: 'video' }, streams: []})
    socket.onmessage?.({ data: JSON.stringify({ event: 'system/disconnect' }) })

    expect(connectionListeners.onState).toHaveBeenCalledTimes(reported)
    expect(connectionListeners.onStream).not.toHaveBeenCalled()
  })

  // A button let go outside the stream, or Alt held through Alt+Tab, stayed down on the
  // remote desktop because its release went elsewhere.
  it('releases every key and button still held', async () => {
    const { connection, channel } = await connected()
    connection.key(PASTE_KEYSYMS.control, true)
    connection.key(PASTE_KEYSYMS.letterV, true)
    connection.key(PASTE_KEYSYMS.letterV, false)
    connection.button(POINTER_BUTTONS.left, true)
    channel.sent = []

    connection.releaseAll()
    expect(channel.sent.map(bytes)).toEqual([
      bytes(encodeKey(PASTE_KEYSYMS.control, false)),
      bytes(encodeButton(POINTER_BUTTONS.left, false)),
    ])

    channel.sent = []
    connection.releaseAll()
    expect(channel.sent).toEqual([])
  })

  // The clipboard went over the signaling socket and the keys over the data channel, so the
  // keys could reach Chromium first and paste the old clipboard.
  it('presses control and V only once Neko confirms the clipboard holds the text', async () => {
    const { connection, socket, channel } = await connected()
    channel.sent = []

    const pasted = connection.paste('hunter2')
    expect(socket.sent.at(-1)).toEqual({ event: 'clipboard/set', payload: { text: 'hunter2' }})
    socket.receive('clipboard/updated', { text: 'an older copy' })
    await Promise.resolve()
    expect(channel.sent).toEqual([])

    socket.receive('clipboard/updated', { text: 'hunter2' })
    await pasted
    expect(channel.sent.map(bytes)).toEqual([
      bytes(encodeKey(PASTE_KEYSYMS.control, true)),
      bytes(encodeKey(PASTE_KEYSYMS.letterV, true)),
      bytes(encodeKey(PASTE_KEYSYMS.letterV, false)),
      bytes(encodeKey(PASTE_KEYSYMS.control, false)),
    ])
  })

  it('pastes anyway when Neko never confirms the clipboard', async () => {
    const { connection, channel } = await connected()
    vi.useFakeTimers()
    channel.sent = []

    const pasted = connection.paste('hunter2')
    await vi.runAllTimersAsync()
    await pasted
    expect(channel.sent).toHaveLength(4)
  })
})
