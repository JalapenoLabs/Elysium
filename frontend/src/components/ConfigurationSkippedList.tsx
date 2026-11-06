// Copyright © 2026 Jalapeno Labs

import type { ConfigurationSkipped } from '../api/routes/agentConfigurationRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Misc
import { skippedKindLabelKeys, skipReasonLabelKeys } from './agentConfigurationPresentation'

type Props = {
  skipped: ConfigurationSkipped[]
}

// What a checkout of the configuration repository held that an agent's container cannot run,
// and so left out: each as the repository names it, why, and what decided it.
export function ConfigurationSkippedList(props: Props) {
  const { t } = useTranslation('llms')

  if (!props.skipped.length) {
    return <p className='text-sm opacity-70'>{t('agentConfiguration.nothingSkipped')}</p>
  }

  return <ul className='flex flex-col gap-2 text-sm'>{
    props.skipped.map((skipped, index) => <li
      // The repository may list the same thing twice, so the place is part of the key.
      key={`${index}-${skipped.kind}-${skipped.name}`}
      className='min-w-0'
    >
      <span className='opacity-70'>{t(skippedKindLabelKeys[skipped.kind])}</span>{' '}
      <span className='break-all font-mono'>{skipped.name}</span>
      <p className='opacity-80'>
        {t(skipReasonLabelKeys[skipped.reason])}
        {skipped.detail && <span className='ml-1 break-all font-mono text-xs opacity-80'>{
          skipped.detail
        }</span>}
      </p>
    </li>)
  }</ul>
}
