// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Misc
import { RUNS_CLOCK_TICK_MS } from '../../constants'
import { useNow } from '../../hooks/useNow'
import { durationBetween } from './runPresentation'

type Props = {
  startedAt: string | null
  finishedAt: string | null
  // Whether it is still going, so the duration counts up each second.
  isLive: boolean
}

// How long a run or a step took, or has taken so far; nothing before it started.
export function RunDuration(props: Props) {
  const { t } = useTranslation('runs')
  const now = useNow(props.isLive
    ? RUNS_CLOCK_TICK_MS
    : null)
  const duration = durationBetween(props.startedAt, props.finishedAt, now)

  if (!duration) {
    return null
  }
  return <span className='tabular-nums'>{
    t(duration.key, duration.values)
  }</span>
}
