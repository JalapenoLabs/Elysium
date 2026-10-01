// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsAdmin } from '../../../store/authSlice'
import { useAppSelector } from '../../../store/hooks'

// User interface
import { Breadcrumbs } from '@heroui/react'
import { ConnectedGrants } from './ConnectedGrants'
import { ConnectMcpClient } from './ConnectMcpClient'
import { RegisteredClients } from './RegisteredClients'

// Misc
import { UrlTree } from '../../../urls'

// Connected apps: how to connect an MCP client such as Claude Code or Codex, the ones this person
// connected, and, for admins, every client that registered. See docs/mcp.md.
export function ConnectedAppsPage() {
  const { t } = useTranslation([ 'oauth', 'settings' ])
  const isAdmin = useAppSelector(selectIsAdmin)

  return <div className='container'>
    <Breadcrumbs className='compact'>
      <Breadcrumbs.Item href={UrlTree.settings}>{t('settings:title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item>{t('connectedApps.title')}</Breadcrumbs.Item>
    </Breadcrumbs>
    <h1 className='relaxed text-3xl font-bold'>{
      t('connectedApps.title')
    }</h1>

    <ConnectMcpClient />
    <ConnectedGrants />
    {isAdmin && <RegisteredClients />}
  </div>
}
