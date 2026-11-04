// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// Redux
import { selectIsAdmin } from '../../../store/authSlice'
import { useAppSelector } from '../../../store/hooks'

// User interface
import { Chip, Tabs } from '@heroui/react'
import { SettingsPage } from '../SettingsPage'
import { ConnectedGrants } from './ConnectedGrants'
import { ConnectMcpClient } from './ConnectMcpClient'
import { RegisteredClients } from './RegisteredClients'

// Misc
import { listOAuthClients, listOAuthGrants } from '../../../api/routes/oauthRoutes'

// `/settings/mcp`: how to connect an MCP client such as Claude Code or Codex, then, on tabs, the
// ones this person connected and, for admins, every client that registered. Both lists load
// with the page, so each tab shows its count before it is opened. Nothing streams for them,
// so a list is fetched again after a change. See docs/mcp.md.
export function McpPage() {
  const { t } = useTranslation('oauth')
  const isAdmin = useAppSelector(selectIsAdmin)
  const [ tab, setTab ] = useState<Key>('grants')
  const grants = useSWR('v1/oauth/grants', listOAuthGrants)
  // Only admins may list every client; for anyone else the key is null and nothing loads.
  const clients = useSWR(
    isAdmin
      ? 'v1/oauth/clients'
      : null,
    listOAuthClients,
  )

  // Someone who stops being an admin keeps the tab they can still see.
  const shownTab = isAdmin
    ? tab
    : 'grants'

  return <SettingsPage title={t('mcp.title')}>
    <ConnectMcpClient />

    <Tabs className='relaxed' variant='secondary' selectedKey={shownTab} onSelectionChange={setTab}>
      <Tabs.ListContainer>
        <Tabs.List aria-label={t('mcp.title')}>
          <Tabs.Tab id='grants'>
            <span>{t('mcp.grants.title')}</span>
            {grants.data && <Chip size='sm' variant='soft' className='ml-2'>{
              grants.data.grants.length
            }</Chip>}
            <Tabs.Indicator />
          </Tabs.Tab>
          {isAdmin && <Tabs.Tab id='clients'>
            <span>{t('mcp.clients.title')}</span>
            {clients.data && <Chip size='sm' variant='soft' className='ml-2'>{
              clients.data.clients.length
            }</Chip>}
            <Tabs.Indicator />
          </Tabs.Tab>}
        </Tabs.List>
      </Tabs.ListContainer>
    </Tabs>

    {shownTab === 'grants' && <ConnectedGrants
      grants={grants.data?.grants}
      hasFailed={Boolean(grants.error)}
      onChanged={() => grants.mutate()}
    />}
    {shownTab === 'clients' && <RegisteredClients
      clients={clients.data?.clients}
      hasFailed={Boolean(clients.error)}
      onChanged={() => clients.mutate()}
    />}
  </SettingsPage>
}
