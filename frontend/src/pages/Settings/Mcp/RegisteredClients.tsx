// Copyright © 2026 Jalapeno Labs

import type { OAuthClient } from '../../../api/routes/oauthRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Spinner } from '@heroui/react'
import { LuBlocks } from 'react-icons/lu'

// Misc
import { formatDate } from '../../../format/dates'
import { deleteOAuthClient } from '../../../api/routes/oauthRoutes'
import { useConfirm } from '../../../hooks/useConfirm'
import { clientHost } from '../../OAuth/oauthPresentation'
import { toast } from '../../../toast/toast'

type Props = {
  // Undefined until the list has loaded.
  clients: OAuthClient[] | undefined
  hasFailed: boolean
  // Fetches the list again after a client was deleted.
  onChanged: () => Promise<unknown>
}

// Every MCP client that registered itself, for admins, with Delete. Deleting one disconnects it
// for everyone; it can register again.
export function RegisteredClients(props: Props) {
  const { t } = useTranslation('oauth')
  const confirm = useConfirm()

  function remove(client: OAuthClient) {
    const name = client.name || t('mcp.unnamed')
    confirm({
      title: t('mcp.clients.confirmTitle', { name }),
      message: t('mcp.clients.confirmMessage', { name }),
      confirmText: t('mcp.clients.delete'),
      isIrreversible: true,
      onConfirm: async () => {
        try {
          await deleteOAuthClient(client.id)
          toast.success(t('mcp.clients.deleted', { name }))
          await props.onChanged()
        }
        catch (failure) {
          console.debug('RegisteredClients could not delete a client', { failure })
          toast.danger(t('mcp.clients.deleteError', { name }))
          throw failure
        }
      },
    })
  }

  if (props.hasFailed) {
    return <p className='text-sm text-danger'>{t('mcp.clients.loadError')}</p>
  }
  if (!props.clients) {
    return <Spinner size='sm' />
  }
  if (!props.clients.length) {
    return <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
      t('mcp.clients.empty')
    }</p>
  }

  return <ul className='flex flex-col gap-2'>{
    props.clients.map((client) => {
      const host = clientHost(client)
      return <li key={client.id} className='level gap-3 rounded-lg bg-surface-secondary px-4 py-3'>
        <div className='level-left min-w-0 gap-3'>
          <LuBlocks className='size-5 shrink-0 opacity-70' aria-hidden />
          <div className='min-w-0'>
            <p className='truncate text-sm font-medium'>{client.name || t('mcp.unnamed')}</p>
            <p className='truncate text-xs opacity-60'>{
              [
                host,
                client.createdAt && t('mcp.clients.registeredOn', {
                  date: formatDate(client.createdAt),
                }),
              ].filter(Boolean).join(' · ')
            }</p>
            <p className='truncate font-mono text-xs opacity-40'>{client.id}</p>
          </div>
        </div>
        <Button size='sm' variant='ghost' onPress={() => remove(client)}>
          <span>{t('mcp.clients.delete')}</span>
        </Button>
      </li>
    })
  }</ul>
}
