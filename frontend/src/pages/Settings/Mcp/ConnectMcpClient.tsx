// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { CopyableValue } from './CopyableValue'

// Misc
import { connectCommandByClient, mcpServerName, mcpServerUrl } from './mcpPresentation'

// How to point an MCP client at Elysium: the server's address, and the command for each client
// people use most. The client then opens a browser here to sign in and approve.
export function ConnectMcpClient() {
  const { t } = useTranslation([ 'oauth', 'common' ])
  const url = mcpServerUrl(window.location.origin)
  const name = mcpServerName(t('common:brand.name'))

  return <section className='relaxed rounded-xl border border-separator p-6'>
    <h2 className='compact text-lg font-semibold'>{t('mcp.connect.title')}</h2>
    <CopyableValue label='URL' value={url} />
    <CopyableValue
      label={t('mcp.connect.claudeCode')}
      value={connectCommandByClient.claudeCode(name, url)}
    />
    <CopyableValue
      label={t('mcp.connect.codex')}
      value={connectCommandByClient.codex(name, url)}
    />
  </section>
}
