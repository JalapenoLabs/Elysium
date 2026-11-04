// Copyright © 2026 Jalapeno Labs

import type { ClaudeAccount } from '../../../api/routes/llmRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Meter } from '@heroui/react'

// Misc
import { formatShortDateTime } from '../../../format/dates'
import { getUsageLevel, usageLevelColors } from './llmPresentation'

type Props = {
  // Only a signed-in account reports usage; Anthropic does not for keys or setup-tokens.
  account: ClaudeAccount | null
  window: 'fiveHour' | 'sevenDay'
  // Names the meter for screen readers. In a table its column header names it for everyone.
  label: string
}

// One of an account's usage windows: how much of it is used, as a bar under its percentage, and
// below them when it resets in the viewer's time zone.
export function ClaudeUsageWindow(props: Props) {
  const { t } = useTranslation('llms')

  if (!props.account) {
    return <span className='text-sm opacity-70'>{t('usage.notAvailable')}</span>
  }

  const usage = props.account.usage
  if (!usage.readAt) {
    return <span className='text-sm opacity-70'>{
      props.account.usageError
        ? t('usage.readFailed')
        : t('usage.notRead')
    }</span>
  }

  const window = usage[props.window]
  if (!window) {
    return <span className='text-sm opacity-70'>{t('usage.notAvailable')}</span>
  }

  return <div className='flex min-w-32 flex-col gap-1'>
    <Meter
      aria-label={props.label}
      size='sm'
      value={Math.min(100, Math.round(window.utilization))}
      color={usageLevelColors[getUsageLevel(window)]}
    >
      {/* Its own line, left aligned: the column header is its label. */}
      <Meter.Output className='text-xs [grid-area:label]' />
      <Meter.Track>
        <Meter.Fill />
      </Meter.Track>
    </Meter>
    {window.resetsAt && <span className='text-xs opacity-70'>{
      t('usage.resets', { when: formatShortDateTime(window.resetsAt) })
    }</span>}
  </div>
}
