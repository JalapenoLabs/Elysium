// Copyright © 2026 Jalapeno Labs

import type { ConfigurationHold } from '../api/routes/agentConfigurationRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Alert, Link } from '@heroui/react'

// Misc
import { UrlTree } from '../urls'
import { holdAlertStatuses, holdLabelKeys } from './agentConfigurationPresentation'

type Props = {
  hold: ConfigurationHold
}

// Why workers and Project Managers wait: no configuration repository, one being fetched, or one
// that could not be fetched, with what it said. Shown where the waiting work is, linking to where
// an admin fixes it.
export function ConfigurationHoldAlert(props: Props) {
  const { t } = useTranslation('llms')

  return <Alert status={holdAlertStatuses[props.hold.reason]} className='relaxed max-w-3xl'>
    <Alert.Indicator />
    <Alert.Content>
      <Alert.Title>{t(holdLabelKeys[props.hold.reason])}</Alert.Title>
      <Alert.Description>
        {props.hold.reason === 'failed' && <span className='block whitespace-pre-wrap font-mono text-xs'>{
          props.hold.error
        }</span>}
        <Link href={UrlTree.settingsLlmsAgentConfiguration} className='text-link'>{
          t('agentConfiguration.title')
        }</Link>
      </Alert.Description>
    </Alert.Content>
  </Alert>
}
