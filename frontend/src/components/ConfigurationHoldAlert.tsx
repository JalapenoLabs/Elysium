// Copyright © 2026 Jalapeno Labs

import type { ConfigurationHold } from '../api/routes/agentConfigurationRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Alert, Link } from '@heroui/react'

// Misc
import { formatRelativeTime } from '../format/dates'
import { useNow } from '../hooks/useNow'
import { UrlTree } from '../urls'
import { holdAlertStatuses, holdLabelKeys } from './agentConfigurationPresentation'

type Props = {
  hold: ConfigurationHold
  // Whether it links to Agent configuration, which the settings page itself leaves out.
  linksToSettings?: boolean
}

// Why workers and Project Managers wait: no configuration repository, one being fetched, or one
// that could not be fetched, with what it said. Shown where the waiting work is, linking to where
// an admin fixes it.
export function ConfigurationHoldAlert(props: Props) {
  const { t } = useTranslation('llms')
  const now = useNow()
  const {
    linksToSettings = true,
  } = props

  return <Alert status={holdAlertStatuses[props.hold.reason]} className='relaxed max-w-3xl'>
    <Alert.Indicator />
    <Alert.Content>
      <Alert.Title>{t(holdLabelKeys[props.hold.reason])}</Alert.Title>
      <Alert.Description>
        {props.hold.reason === 'failed' && <>
          <span className='block opacity-80'>{
            t('agentConfiguration.failedAt', { time: formatRelativeTime(props.hold.failedAt, now) })
          }</span>
          <span className='block whitespace-pre-wrap break-all font-mono text-xs'>{props.hold.error}</span>
        </>}
        {linksToSettings && <Link href={UrlTree.settingsLlmsAgentConfiguration} className='text-link'>{
          t('agentConfiguration.title')
        }</Link>}
      </Alert.Description>
    </Alert.Content>
  </Alert>
}
