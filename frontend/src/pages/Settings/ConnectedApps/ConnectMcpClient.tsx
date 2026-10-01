// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { SecuritySection } from '../Security/SecuritySection'
import { CopyableValue } from './CopyableValue'

// Misc
import { connectCommandByClient, mcpServerName, mcpServerUrl } from './connectedAppsPresentation'

// How to point an MCP client at Elysium: the server's address, and the command for each client
// people use most. The client then opens a browser here to sign in and approve.
export function ConnectMcpClient() {
  const { t } = useTranslation([ 'oauth', 'common' ])
  const url = mcpServerUrl(window.location.origin)
  const name = mcpServerName(t('common:brand.name'))

  return <SecuritySection
    title={t('connectedApps.connect.title')}
    description={t('connectedApps.connect.description')}
  >
    <div className='mt-4'>
      <CopyableValue label='URL' value={url} />
      <CopyableValue
        label={t('connectedApps.connect.claudeCode')}
        value={connectCommandByClient.claudeCode(name, url)}
      />
      <CopyableValue
        label={t('connectedApps.connect.codex')}
        value={connectCommandByClient.codex(name, url)}
      />
      <p className='text-xs opacity-60'>{t('connectedApps.connect.browserNote')}</p>
    </div>
  </SecuritySection>
}
