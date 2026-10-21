// Copyright © 2026 Jalapeno Labs

// Neko's input messages, sent on the WebRTC data channel the browser's offer opens. Each is a
// big-endian header (event u8, body length u16) and a body, as Neko 3.1.6's server reads them
// in `server/internal/webrtc/handler.go` and `payload/receive.go`. See docs/browser.md.

const HEADER_BYTES = 3

const OPERATION = {
  move: 0x01,
  scroll: 0x02,
  keyDown: 0x03,
  keyUp: 0x04,
  buttonDown: 0x05,
  buttonUp: 0x06,
} as const

// X11 pointer buttons, which Neko sends to the desktop as they are.
export const POINTER_BUTTONS = {
  left: 1,
  middle: 2,
  right: 3,
} as const

// `MouseEvent.button` numbers the same three buttons differently.
export const pointerButtonByMouseButton: Record<number, number> = {
  0: POINTER_BUTTONS.left,
  1: POINTER_BUTTONS.middle,
  2: POINTER_BUTTONS.right,
}

function message(operation: number, bodyBytes: number, writeBody: (body: DataView) => void) {
  const buffer = new ArrayBuffer(HEADER_BYTES + bodyBytes)
  const view = new DataView(buffer)
  view.setUint8(0, operation)
  view.setUint16(1, bodyBytes)
  writeBody(new DataView(buffer, HEADER_BYTES))
  return buffer
}

// Moves the pointer to (x, y) in the desktop's pixels.
export function encodeMove(x: number, y: number) {
  return message(OPERATION.move, 4, (body) => {
    body.setUint16(0, x)
    body.setUint16(2, y)
  })
}

// Scrolls by whole steps; positive y scrolls down. Control turns a scroll into a zoom.
export function encodeScroll(deltaX: number, deltaY: number, isControlHeld: boolean) {
  return message(OPERATION.scroll, 5, (body) => {
    body.setInt16(0, deltaX)
    body.setInt16(2, deltaY)
    body.setUint8(4, isControlHeld
      ? 1
      : 0)
  })
}

export function encodeKey(keysym: number, isDown: boolean) {
  const operation = isDown
    ? OPERATION.keyDown
    : OPERATION.keyUp
  return message(operation, 4, (body) => body.setUint32(0, keysym))
}

export function encodeButton(button: number, isDown: boolean) {
  const operation = isDown
    ? OPERATION.buttonDown
    : OPERATION.buttonUp
  return message(operation, 4, (body) => body.setUint32(0, button))
}
