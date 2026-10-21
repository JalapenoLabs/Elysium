// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { encodeButton, encodeKey, encodeMove, encodeScroll, POINTER_BUTTONS } from './nekoInput'

function bytes(buffer: ArrayBuffer) {
  return [ ...new Uint8Array(buffer) ]
}

// Byte layouts as Neko 3.1.6's server reads them: a big-endian header of the operation
// (u8) and the body's length (u16), then the body.
describe('Neko input messages', () => {
  it('moves the pointer with two big-endian u16 coordinates', () => {
    expect(bytes(encodeMove(1920, 1080))).toEqual([ 0x01, 0x00, 0x04, 0x07, 0x80, 0x04, 0x38 ])
  })

  it('scrolls with signed deltas and the control flag', () => {
    expect(bytes(encodeScroll(0, -3, true))).toEqual([ 0x02, 0x00, 0x05, 0x00, 0x00, 0xff, 0xfd, 0x01 ])
    expect(bytes(encodeScroll(2, 0, false))).toEqual([ 0x02, 0x00, 0x05, 0x00, 0x02, 0x00, 0x00, 0x00 ])
  })

  it('presses and releases keys by their u32 keysym', () => {
    expect(bytes(encodeKey(0xff0d, true))).toEqual([ 0x03, 0x00, 0x04, 0x00, 0x00, 0xff, 0x0d ])
    expect(bytes(encodeKey(0x01000142, false))).toEqual([ 0x04, 0x00, 0x04, 0x01, 0x00, 0x01, 0x42 ])
  })

  it('presses and releases pointer buttons by X11 number', () => {
    expect(bytes(encodeButton(POINTER_BUTTONS.left, true))).toEqual([ 0x05, 0x00, 0x04, 0x00, 0x00, 0x00, 0x01 ])
    expect(bytes(encodeButton(POINTER_BUTTONS.right, false))).toEqual([ 0x06, 0x00, 0x04, 0x00, 0x00, 0x00, 0x03 ])
  })
})
