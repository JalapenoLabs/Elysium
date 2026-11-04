// Copyright © 2026 Jalapeno Labs

import type { ModelPrice } from '../../../api/routes/modelSettingRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Misc
import { formatMoney } from '../../../format/numbers'

type Props = {
  model: string
  prices: ModelPrice[]
}

// What `model` costs per million tokens, input over output, so one model's price reads beside
// another's. A model typed by hand has no price the API knows.
export function ModelPriceText(props: Props) {
  const { t } = useTranslation('models')
  const price = props.prices.find((candidate) => candidate.model === props.model.trim())

  if (!price) {
    return <span className='opacity-60'>{t('price.unknown')}</span>
  }

  return <div className='flex flex-col'>
    <span>{
      t('price.input', { amount: formatMoney(price.inputCentsPerMillionTokens / 100, 'USD') })
    }</span>
    <span className='text-xs opacity-60'>{
      t('price.output', { amount: formatMoney(price.outputCentsPerMillionTokens / 100, 'USD') })
    }</span>
  </div>
}
