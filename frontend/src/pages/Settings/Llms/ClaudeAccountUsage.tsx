// Copyright © 2026 Jalapeno Labs

import type { ClaudeAccount, UsageWindow } from '../../../api/routes/llmRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Meter } from '@heroui/react'

// Misc
import { getUsageLevel, usageLevelColors } from './llmPresentation'

type Props = {
  account: ClaudeAccount
}

// An account's usage windows, each as a bar with when it resets in the viewer's time zone.
export function ClaudeAccountUsage(props: Props) {
  const { t, i18n } = useTranslation('llms')
  const usage = props.account.usage

  if (!usage.readAt) {
    return <span className='text-sm opacity-70'>{
      props.account.usageError
        ? t('usage.readFailed')
        : t('usage.notRead')
    }</span>
  }

  // Timestamps arrive as UTC; this is where they become the viewer's local time.
  const resetFormatter = new Intl.DateTimeFormat(i18n.language, {
    weekday: 'short',
    hour: 'numeric',
    minute: '2-digit',
  })
  const windows = [
    { key: 'fiveHour', label: t('usage.fiveHour'), window: usage.fiveHour },
    { key: 'sevenDay', label: t('usage.sevenDay'), window: usage.sevenDay },
  ] as const

  return <div className='flex min-w-40 flex-col gap-2'>{
    windows.map(({ key, label, window }) => window && <UsageMeter
      key={key}
      label={label}
      window={window}
      resetsAt={window.resetsAt
        ? t('usage.resets', { when: resetFormatter.format(new Date(window.resetsAt)) })
        : null}
    />)
  }</div>
}

type MeterProps = {
  label: string
  window: UsageWindow
  resetsAt: string | null
}

function UsageMeter(props: MeterProps) {
  const percent = Math.min(100, Math.round(props.window.utilization))

  return <Meter
    aria-label={props.label}
    size='sm'
    value={percent}
    color={usageLevelColors[getUsageLevel(props.window)]}
  >
    <div className='flex justify-between gap-2 text-xs'>
      <span>{props.label}</span>
      <Meter.Output />
    </div>
    <Meter.Track>
      <Meter.Fill />
    </Meter.Track>
    {props.resetsAt && <span className='text-xs opacity-70'>{props.resetsAt}</span>}
  </Meter>
}
