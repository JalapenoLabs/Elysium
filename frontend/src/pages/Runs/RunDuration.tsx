// Copyright © 2026 Jalapeno Labs

// Misc
import { RUNS_CLOCK_TICK_MS } from '../../constants'
import { formatDuration } from '../../format/durations'
import { useNow } from '../../hooks/useNow'

type Props = {
  startedAt: string | null
  finishedAt: string | null
  // Whether it is still going, so the duration counts up each second.
  isLive: boolean
}

// How long a run or a step took, or has taken so far; nothing before it started.
export function RunDuration(props: Props) {
  const now = useNow(props.isLive
    ? RUNS_CLOCK_TICK_MS
    : null)

  if (!props.startedAt) {
    return null
  }
  const end = props.finishedAt
    ? Date.parse(props.finishedAt)
    : now
  return <span className='tabular-nums'>{
    formatDuration(end - Date.parse(props.startedAt))
  }</span>
}
