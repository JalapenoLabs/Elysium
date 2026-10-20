// Copyright © 2026 Jalapeno Labs

// One viewer's connection to a browser the host streams: Neko's signaling over Elysium's
// stream route, the WebRTC peer that carries the video, and the data channel that carries
// input. Neko's own client cannot run under the web app's CSP, so this speaks its protocol
// directly (`server/pkg/types/event` and `message` in Neko 3.1.6). See docs/browser.md.

// Misc
import { encodeButton, encodeKey, encodeMove, encodeScroll } from './nekoInput'
import { PASTE_KEYSYMS } from './keysyms'

export type ConnectionState = 'connecting' | 'connected' | 'disconnected'

export type ScreenSize = {
  width: number
  height: number
}

// The remote pointer's shape, as a CSS cursor value.
export type RemoteCursor = {
  url: string
  hotX: number
  hotY: number
}

export type ConnectionListeners = {
  onState: (state: ConnectionState) => void
  onStream: (stream: MediaStream) => void
  onScreenSize: (size: ScreenSize) => void
  onCursor: (cursor: RemoteCursor) => void
}

type NekoMessage = {
  event: string
  payload?: unknown
}

type SignalProvide = {
  sdp: string
  iceservers?: RTCIceServer[]
}

type SystemInit = {
  screen_size?: ScreenSize
}

// The data channel's server-to-viewer operation for a new cursor image (`payload/send.go`).
const CURSOR_IMAGE_OPERATION = 0x02
// The header (operation, length) and the image's size and hot spot before its PNG bytes.
const CURSOR_IMAGE_PREFIX_BYTES = 3 + 8

export class NekoConnection {
  private readonly url: string
  private readonly listeners: ConnectionListeners
  private socket: WebSocket | null = null
  private peer: RTCPeerConnection | null = null
  private channel: RTCDataChannel | null = null
  // Candidates can arrive before the offer, and wait for it.
  private pendingCandidates: RTCIceCandidateInit[] = []
  private cursorUrl: string | null = null

  constructor(url: string, listeners: ConnectionListeners) {
    this.url = url
    this.listeners = listeners
  }

  open() {
    this.listeners.onState('connecting')
    const socket = new WebSocket(this.url)
    this.socket = socket
    socket.onmessage = (message) => {
      void this.receive(message.data)
    }
    socket.onclose = () => this.close()
    socket.onerror = (error) => {
      console.debug('NekoConnection socket failed', { error })
    }
  }

  close() {
    this.socket?.close()
    this.peer?.close()
    this.socket = null
    this.peer = null
    this.channel = null
    this.pendingCandidates = []
    if (this.cursorUrl) {
      URL.revokeObjectURL(this.cursorUrl)
      this.cursorUrl = null
    }
    this.listeners.onState('disconnected')
  }

  move(x: number, y: number) {
    this.sendInput(encodeMove(x, y))
  }

  scroll(deltaX: number, deltaY: number, isControlHeld: boolean) {
    this.sendInput(encodeScroll(deltaX, deltaY, isControlHeld))
  }

  key(keysym: number, isDown: boolean) {
    this.sendInput(encodeKey(keysym, isDown))
  }

  button(button: number, isDown: boolean) {
    this.sendInput(encodeButton(button, isDown))
  }

  // Puts `text` on the remote clipboard and presses control and V, so a password manager on
  // the viewer's own machine fills the remote page.
  paste(text: string) {
    this.send({ event: 'clipboard/set', payload: { text }})
    this.key(PASTE_KEYSYMS.control, true)
    this.key(PASTE_KEYSYMS.letterV, true)
    this.key(PASTE_KEYSYMS.letterV, false)
    this.key(PASTE_KEYSYMS.control, false)
  }

  private send(message: NekoMessage) {
    if (this.socket?.readyState !== WebSocket.OPEN) {
      console.debug('NekoConnection dropped a message while the socket is closed', { event: message.event })
      return
    }
    this.socket.send(JSON.stringify(message))
  }

  // Input is only meaningful while the data channel is open; a gesture before then is lost,
  // as it would be on a frozen screen.
  private sendInput(buffer: ArrayBuffer) {
    if (this.channel?.readyState !== 'open') {
      return
    }
    this.channel.send(buffer)
  }

  private async receive(data: unknown) {
    if (typeof data !== 'string') {
      console.debug('NekoConnection ignored a binary signaling message')
      return
    }
    const message: NekoMessage = JSON.parse(data)

    if (message.event === 'system/init') {
      const init = message.payload as SystemInit
      if (init.screen_size) {
        this.listeners.onScreenSize(init.screen_size)
      }
      this.send({ event: 'signal/request', payload: { video: {}, audio: {}}})
      // Neko hosts implicitly: the first viewer to ask takes the pointer and keyboard.
      this.send({ event: 'control/request' })
      return
    }

    if (message.event === 'screen/updated') {
      this.listeners.onScreenSize(message.payload as ScreenSize)
      return
    }

    if (message.event === 'signal/provide') {
      await this.answer(message.payload as SignalProvide)
      return
    }

    if (message.event === 'signal/candidate') {
      const candidate = message.payload as RTCIceCandidateInit
      if (this.peer?.remoteDescription) {
        await this.peer.addIceCandidate(candidate)
        return
      }
      this.pendingCandidates.push(candidate)
      return
    }

    if (message.event === 'system/heartbeat') {
      this.send({ event: 'client/heartbeat' })
      return
    }

    if (message.event === 'system/disconnect') {
      this.close()
    }
  }

  private async answer(provide: SignalProvide) {
    const peer = new RTCPeerConnection({ iceServers: provide.iceservers ?? []})
    this.peer = peer

    peer.onicecandidate = (event) => {
      if (event.candidate) {
        this.send({ event: 'signal/candidate', payload: event.candidate.toJSON() })
      }
    }
    peer.onconnectionstatechange = () => {
      if (peer.connectionState === 'connected') {
        this.listeners.onState('connected')
      }
      if (peer.connectionState === 'failed') {
        console.debug('NekoConnection peer connection failed')
        this.close()
      }
    }
    peer.ontrack = (event) => {
      if (event.track.kind !== 'video') {
        return
      }
      this.listeners.onStream(event.streams[0] ?? new MediaStream([ event.track ]))
    }
    peer.ondatachannel = (event) => {
      this.channel = event.channel
      this.channel.binaryType = 'arraybuffer'
      this.channel.onmessage = (message) => this.receiveData(message.data)
    }

    await peer.setRemoteDescription({ type: 'offer', sdp: provide.sdp })
    for (const candidate of this.pendingCandidates) {
      await peer.addIceCandidate(candidate)
    }
    this.pendingCandidates = []

    const answer = await peer.createAnswer()
    await peer.setLocalDescription(answer)
    this.send({ event: 'signal/answer', payload: { sdp: answer.sdp }})
  }

  private receiveData(data: unknown) {
    if (!(data instanceof ArrayBuffer) || data.byteLength < CURSOR_IMAGE_PREFIX_BYTES) {
      return
    }
    const view = new DataView(data)
    if (view.getUint8(0) !== CURSOR_IMAGE_OPERATION) {
      return
    }

    const image = new Blob([ data.slice(CURSOR_IMAGE_PREFIX_BYTES) ], { type: 'image/png' })
    if (this.cursorUrl) {
      URL.revokeObjectURL(this.cursorUrl)
    }
    this.cursorUrl = URL.createObjectURL(image)
    this.listeners.onCursor({
      url: this.cursorUrl,
      hotX: view.getUint16(7),
      hotY: view.getUint16(9),
    })
  }
}
