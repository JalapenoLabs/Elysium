// Copyright © 2026 Jalapeno Labs

import type { PageTab } from '../../../components/PageTabs'

// Core
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// Redux
import { selectIsAdmin } from '../../../store/authSlice'
import { useAppSelector } from '../../../store/hooks'

// User interface
import { Tabs } from '@heroui/react'
import { PageTabs } from '../../../components/PageTabs'
import { SettingsPage } from '../SettingsPage'
import { ConnectedGrants } from './ConnectedGrants'
import { ConnectMcpClient } from './ConnectMcpClient'
import { RegisteredClients } from './RegisteredClients'

// Misc
import { listOAuthClients, listOAuthGrants } from '../../../api/routes/oauthRoutes'
import { useTabParam } from '../../../hooks/useTabParam'

type McpTab = 'grants' | 'clients'

// `/settings/mcp`: how to connect an MCP client such as Claude Code or Codex, then, on tabs, the
// ones this person connected and, for admins, every client that registered. Both lists load
// with the page, so each tab shows its count before it is opened. Nothing streams for them,
// so a list is fetched again after a change. The open tab lives in the address. See docs/mcp.md.
export function McpPage() {
  const { t } = useTranslation('oauth')
  const isAdmin = useAppSelector(selectIsAdmin)
  const grants = useSWR('v1/oauth/grants', listOAuthGrants)
  // Only admins may list every client; for anyone else the key is null and nothing loads.
  const clients = useSWR(
    isAdmin
      ? 'v1/oauth/clients'
      : null,
    listOAuthClients,
  )

  const tabs: PageTab<McpTab>[] = [
    { id: 'grants', label: t('mcp.grants.title'), count: grants.data?.grants.length ?? null },
  ]
  if (isAdmin) {
    tabs.push({ id: 'clients', label: t('mcp.clients.title'), count: clients.data?.clients.length ?? null })
  }
  // Someone who stops being an admin keeps the tab they can still see.
  const [ selectedTab, selectTab ] = useTabParam(tabs.map((tab) => tab.id))

  return <SettingsPage title={t('mcp.title')}>
    <ConnectMcpClient />

    <PageTabs
      label={t('mcp.title')}
      selectedKey={selectedTab}
      onSelectionChange={selectTab}
      tabs={tabs}
    >
      <Tabs.Panel id='grants'>
        <ConnectedGrants
          grants={grants.data?.grants}
          hasFailed={Boolean(grants.error)}
          onChanged={() => grants.mutate()}
        />
      </Tabs.Panel>
      {isAdmin && <Tabs.Panel id='clients'>
        <RegisteredClients
          clients={clients.data?.clients}
          hasFailed={Boolean(clients.error)}
          onChanged={() => clients.mutate()}
        />
      </Tabs.Panel>}
    </PageTabs>
  </SettingsPage>
}
