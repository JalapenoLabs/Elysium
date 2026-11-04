// Copyright © 2026 Jalapeno Labs

import type { RecordedUsage } from '../../../api/routes/modelSettingRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Misc
import { formatMoney } from '../../../format/numbers'

// Claude Code reports cost in millionths of a dollar.
const MICROS_PER_DOLLAR = 1_000_000

type Props = {
  // `null` where the kind's turns record no cost.
  usage: RecordedUsage | null
}

// What a kind's turns spent, over how many turns, or that its turns keep no record of it.
export function RecordedUsageText(props: Props) {
  const { t } = useTranslation('models')

  if (!props.usage) {
    return <span className='opacity-60'>{t('usage.notRecorded')}</span>
  }

  return <div className='flex flex-col'>
    <span>{
      formatMoney(props.usage.costUsdMicros / MICROS_PER_DOLLAR, 'USD')
    }</span>
    <span className='text-xs opacity-60'>{
      t('usage.turns', { count: props.usage.turns })
    }</span>
  </div>
}
