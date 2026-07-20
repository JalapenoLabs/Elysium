// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'
import moment from 'moment-timezone'

// Lib
import { create } from '@bufbuild/protobuf'

// Misc
import { Timestamp } from './timestamp'
import { TimestampSchema } from '../generated/common_pb'
import { DEFAULT_TIMEZONE } from './constants'

// A fixed instant with sub-second precision: 2026-07-07T15:30:00.250Z.
const instant = new Date('2026-07-07T15:30:00.250Z')

describe('Timestamp.fromDate', () => {
  it('splits the instant into UTC epoch seconds plus nanos', () => {
    const timestamp = Timestamp.fromDate(instant, 'Asia/Tokyo')

    expect(timestamp.proto.epochSeconds).toBe(BigInt(Math.floor(instant.getTime() / 1000)))
    expect(timestamp.proto.nanos).toBe(250_000_000)
    expect(timestamp.proto.timezone).toBe('Asia/Tokyo')
  })

  it('defaults the display zone without altering the instant', () => {
    const timestamp = Timestamp.fromDate(instant)
    expect(timestamp.proto.timezone).toBe(DEFAULT_TIMEZONE)
    expect(timestamp.toDate().getTime()).toBe(instant.getTime())
  })
})

describe('Timestamp round trips', () => {
  it('toDate reconstructs the exact millisecond instant', () => {
    const timestamp = Timestamp.fromDate(instant, 'America/Denver')
    expect(timestamp.toDate().toISOString()).toBe('2026-07-07T15:30:00.250Z')
  })

  it('fromMoment carries the moment zone and instant through', () => {
    const tokyoMoment = moment(instant).tz('Asia/Tokyo')
    const timestamp = Timestamp.fromMoment(tokyoMoment)

    expect(timestamp.proto.timezone).toBe('Asia/Tokyo')
    expect(timestamp.toDate().getTime()).toBe(instant.getTime())
  })

  it('toMoment renders in the carried zone while staying the same instant', () => {
    const timestamp = Timestamp.fromDate(instant, 'Asia/Tokyo')
    const rendered = timestamp.toMoment()

    // 15:30 UTC = 00:30 next day in Tokyo (+09:00), same absolute instant.
    expect(rendered.format('YYYY-MM-DD HH:mm')).toBe('2026-07-08 00:30')
    expect(rendered.valueOf()).toBe(instant.getTime())
  })

  it('falls back to the default zone when the message carries none', () => {
    const bare = create(TimestampSchema, {
      epochSeconds: BigInt(Math.floor(instant.getTime() / 1000)),
      nanos: 0,
      timezone: '',
    })
    expect(new Timestamp(bare).toMoment().tz()).toBe(DEFAULT_TIMEZONE)
  })
})
