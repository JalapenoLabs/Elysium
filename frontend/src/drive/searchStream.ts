// Copyright © 2026 Jalapeno Labs

import type { SearchFrame } from '../proto/generated/storage_pb'

// Lib
import { create, fromBinary, toBinary } from '@bufbuild/protobuf'

// Misc
import { API_BASE_URL } from '../constants'
import { SearchFrameSchema, SearchRequestSchema } from '../proto/generated/storage_pb'

// The federated-search stream (docs/storage.md): one POST, a chunked response
// of length-delimited SearchFrame messages that interleave across Drives in
// arrival order. Frames are consumed incrementally so fast Drives light up
// while slow ones are still working, and the whole search aborts through the
// caller's AbortSignal.

// Decodes every complete frame in `buffer`. Chunk boundaries land anywhere, so
// a trailing partial varint or partial frame is returned as `rest` for the
// caller to prepend to the next chunk.
export function decodeSearchFrames(buffer: Uint8Array) {
  const frames: SearchFrame[] = []
  let offset = 0

  while (offset < buffer.length) {
    // Varint length prefix (LEB128).
    let length = 0
    let shift = 0
    let cursor = offset
    let byte = 0
    do {
      if (cursor >= buffer.length) {
        return { frames, rest: buffer.slice(offset) } as const
      }
      byte = buffer[cursor]
      length |= (byte & 0x7f) << shift
      shift += 7
      cursor += 1
    } while (byte & 0x80)

    if (cursor + length > buffer.length) {
      return { frames, rest: buffer.slice(offset) } as const
    }

    frames.push(fromBinary(SearchFrameSchema, buffer.subarray(cursor, cursor + length)))
    offset = cursor + length
  }

  return { frames, rest: new Uint8Array(0) } as const
}

type SearchOptions = {
  query: string
  // Empty = every Drive the caller can access across the active org.
  driveIds: string[]
  signal: AbortSignal
  onFrame: (frame: SearchFrame) => void
}

// Streams the search, invoking `onFrame` per decoded frame as bytes arrive.
// Uses fetch directly: Ky's convenience helpers buffer the body, and this is
// exactly the case where the body must be read incrementally.
export async function searchDrives(options: SearchOptions): Promise<void> {
  const response = await fetch(`${API_BASE_URL}/api/v1/search`, {
    method: 'POST',
    credentials: 'include',
    signal: options.signal,
    headers: { 'content-type': 'application/x-protobuf' },
    body: toBinary(SearchRequestSchema, create(SearchRequestSchema, {
      query: options.query,
      driveIds: options.driveIds.map((driveId) => ({ value: driveId })),
    })),
  })

  if (!response.ok || !response.body) {
    throw new Error(`search failed with status ${response.status}`)
  }

  const reader = response.body.getReader()
  let buffer = new Uint8Array(0)

  for (;;) {
    const { done, value } = await reader.read()
    if (done) {
      break
    }

    const combined = new Uint8Array(buffer.length + value.length)
    combined.set(buffer)
    combined.set(value, buffer.length)

    const { frames, rest } = decodeSearchFrames(combined)
    buffer = rest
    for (const frame of frames) {
      options.onFrame(frame)
    }
  }
}
