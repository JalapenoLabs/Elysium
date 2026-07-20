// Copyright © 2026 Jalapeno Labs

import type { Timestamp as TimestampMessage } from '../proto/generated/common_pb'

// Core
import { useMemo } from 'react'

// UI
import { Tooltip } from '@heroui/react'

// Misc
import { Timestamp } from '../proto/helpers/timestamp'
import { resolveDisplayTimezone } from '../proto/helpers/timezone'
import { STANDARD_SHORT_TIME_FORMAT, STANDARD_FULL_TIME_FORMAT } from '../proto/helpers/constants'

type Props = {
  // The instant to render, as the wire `Timestamp` message.
  timestamp: TimestampMessage
  // Render a relative "2 hours ago" instead of an absolute short time. The full
  // absolute instant is always available on hover regardless.
  relative?: boolean
  // The zone to display in. Omit to use the viewer's zone (their saved
  // preference, else browser auto-detect); pass an explicit IANA zone to override.
  timezone?: string
  className?: string
}

// A timestamp that reads short and tells the full story on hover: it shows a
// compact instant (or a relative "2 hours ago") and reveals the unabbreviated
// date, time, and zone in a tooltip. Instants arrive in UTC and are rendered in
// the viewer's zone, never blanket UTC. Uses a native <time> element so the
// machine still gets the ISO instant.
export function OhLookAtTheTime(props: Props) {
  const displayTimezone = props.timezone ?? resolveDisplayTimezone()

  const { date, primary, fullTime } = useMemo(() => {
    const moment = new Timestamp(props.timestamp).toMoment().tz(displayTimezone)
    return {
      date: moment.toDate(),
      primary: props.relative ? moment.fromNow() : moment.format(STANDARD_SHORT_TIME_FORMAT),
      fullTime: moment.format(STANDARD_FULL_TIME_FORMAT),
    }
  }, [ props.timestamp, props.relative, displayTimezone ])

  return <Tooltip content={fullTime}>
    <time dateTime={date.toISOString()} className={props.className}>{ primary }</time>
  </Tooltip>
}
