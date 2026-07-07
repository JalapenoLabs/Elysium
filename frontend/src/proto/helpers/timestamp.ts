// Copyright © 2026 Jalapeno Labs
//
// Ergonomic wrapper over the generated `Timestamp` message.
//
// The generated type is plain data; this class is the dev-friendly front door for
// converting in and out of native `Date` and `moment` values while keeping the
// UTC-canonical, timezone-aware standard from ~/.claude/docs/proto.md intact.

import type { Moment } from 'moment-timezone'
import type { Timestamp as TimestampMessage } from '../generated/common_pb'

// Core
import moment from 'moment-timezone'

// Lib
import { create } from '@bufbuild/protobuf'

// Misc
import { TimestampSchema } from '../generated/common_pb'
import { STANDARD_TIME_FORMAT, DEFAULT_TIMEZONE } from './constants'

const MILLISECONDS_PER_SECOND = 1000
const NANOS_PER_MILLISECOND = 1_000_000

export class Timestamp {
  private readonly message: TimestampMessage

  constructor(message: TimestampMessage) {
    this.message = message
  }

  // Date is millisecond-only, so any sub-millisecond `nanos` are dropped here.
  // Full nanosecond fidelity survives only through the proto message itself.
  static fromDate(date: Date, timezone: string = DEFAULT_TIMEZONE): Timestamp {
    const epochSeconds = BigInt(Math.floor(date.getTime() / MILLISECONDS_PER_SECOND))
    const nanos = date.getMilliseconds() * NANOS_PER_MILLISECOND
    return new Timestamp(create(TimestampSchema, { epochSeconds, nanos, timezone }))
  }

  static fromMoment(value: Moment): Timestamp {
    return Timestamp.fromDate(value.toDate(), value.tz() || DEFAULT_TIMEZONE)
  }

  static now(timezone: string = DEFAULT_TIMEZONE): Timestamp {
    return Timestamp.fromDate(new Date(), timezone)
  }

  // The underlying generated message, for sending back over the wire.
  get proto(): TimestampMessage {
    return this.message
  }

  toDate(): Date {
    const milliseconds = Number(this.message.epochSeconds) * MILLISECONDS_PER_SECOND
      + Math.floor(this.message.nanos / NANOS_PER_MILLISECOND)
    return new Date(milliseconds)
  }

  toMoment(): Moment {
    return moment(this.toDate()).tz(this.message.timezone || DEFAULT_TIMEZONE)
  }

  format(format: string = STANDARD_TIME_FORMAT): string {
    return this.toMoment().format(format)
  }
}
