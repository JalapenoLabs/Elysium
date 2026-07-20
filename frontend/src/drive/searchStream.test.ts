// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'
import { create, toBinary } from '@bufbuild/protobuf'

// Misc
import { decodeSearchFrames } from './searchStream'
import {
  SearchFrame_Status as searchStatus,
  SearchFrameSchema,
} from '../proto/generated/storage_pb'

function encodeFrame(driveName: string, matchNames: string[]): Uint8Array {
  const frame = create(SearchFrameSchema, {
    driveId: 'd1',
    driveName,
    status: searchStatus.SEARCHING,
    matches: matchNames.map((name) => ({ name, path: name })),
  })
  const bytes = toBinary(SearchFrameSchema, frame)

  // Varint length prefix (all test frames are far under 128 bytes... unless a
  // long name pushes one over, which the multi-byte case below relies on).
  const prefix: number[] = []
  let remaining = bytes.length
  do {
    let byte = remaining & 0x7f
    remaining >>>= 7
    if (remaining) {
      byte |= 0x80
    }
    prefix.push(byte)
  } while (remaining)

  const framed = new Uint8Array(prefix.length + bytes.length)
  framed.set(prefix)
  framed.set(bytes, prefix.length)
  return framed
}

function concat(...chunks: Uint8Array[]): Uint8Array {
  const total = chunks.reduce((sum, chunk) => sum + chunk.length, 0)
  const combined = new Uint8Array(total)
  let offset = 0
  for (const chunk of chunks) {
    combined.set(chunk, offset)
    offset += chunk.length
  }
  return combined
}

describe('decodeSearchFrames', () => {
  it('decodes multiple back-to-back frames', () => {
    const buffer = concat(encodeFrame('Local', [ 'a.txt' ]), encodeFrame('S3', [ 'b.txt' ]))
    const { frames, rest } = decodeSearchFrames(buffer)

    expect(frames.map((frame) => frame.driveName)).toEqual([ 'Local', 'S3' ])
    expect(rest.length).toBe(0)
  })

  it('returns a partial trailing frame as rest and completes it next chunk', () => {
    const first = encodeFrame('Local', [ 'a.txt' ])
    const second = encodeFrame('S3', [ 'b.txt' ])
    const splitAt = first.length + 3

    const firstPass = decodeSearchFrames(concat(first, second).slice(0, splitAt))
    expect(firstPass.frames).toHaveLength(1)
    expect(firstPass.rest.length).toBe(3)

    const secondPass = decodeSearchFrames(concat(firstPass.rest, second.slice(3)))
    expect(secondPass.frames).toHaveLength(1)
    expect(secondPass.frames[0].driveName).toBe('S3')
    expect(secondPass.rest.length).toBe(0)
  })

  it('handles a frame large enough for a multi-byte varint prefix', () => {
    const longNames = Array.from({ length: 40 }, (_unused, index) => `file-${index}.txt`)
    const framed = encodeFrame('Big', longNames)
    // Sanity: the payload really is over one varint byte's range.
    expect(framed.length).toBeGreaterThan(128)

    const { frames, rest } = decodeSearchFrames(framed)
    expect(frames).toHaveLength(1)
    expect(frames[0].matches).toHaveLength(40)
    expect(rest.length).toBe(0)
  })

  it('holds a partial varint prefix until more bytes arrive', () => {
    const framed = encodeFrame('Big', Array.from({ length: 40 }, (_unused, index) => `file-${index}.txt`))
    // Slice inside the (multi-byte) varint itself.
    const { frames, rest } = decodeSearchFrames(framed.slice(0, 1))
    expect(frames).toHaveLength(0)
    expect(rest.length).toBe(1)
  })
})
