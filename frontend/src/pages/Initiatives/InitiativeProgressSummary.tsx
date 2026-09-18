// Copyright © 2026 Jalapeno Labs

import type { InitiativeProgress } from '../../api/routes/initiativeRoutes'

// Core
import { useTranslation } from 'react-i18next'

type Props = {
  progress: InitiativeProgress
  // A larger figure for the initiative's own page.
  size?: 'sm' | 'lg'
}

// Progress as resolved and total together, never a percentage alone: added scope shows as a
// bigger total rather than as a bar moving backwards. The bar beside the counts is only a
// picture of them.
export function InitiativeProgressSummary(props: Props) {
  const { t } = useTranslation('initiatives')
  const { resolved, total } = props.progress
  const ratio = total
    ? resolved / total
    : 0

  return <div className='flex min-w-0 items-center gap-3'>
    <span className={props.size === 'lg'
      ? 'text-2xl font-semibold'
      : 'text-sm whitespace-nowrap'}
    >{
      t('progress.counts', { resolved, total })
    }</span>
    <div
      className='h-1.5 min-w-12 flex-1 overflow-hidden rounded-full bg-surface-secondary'
      aria-hidden
    >
      <div className='h-full rounded-full bg-chart-resolved' style={{ width: `${ratio * 100}%` }} />
    </div>
  </div>
}
