// Copyright © 2026 Jalapeno Labs

import type { OAuthClient } from '../../../api/routes/oauthRoutes'

// Core
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { Button, Spinner, toast } from '@heroui/react'
import { LuBlocks } from 'react-icons/lu'
import { SecuritySection } from '../Security/SecuritySection'

// Misc
import { deleteOAuthClient, listOAuthClients } from '../../../api/routes/oauthRoutes'
import { useConfirm } from '../../../hooks/useConfirm'
import { clientHost } from '../../OAuth/oauthPresentation'

const dateFormatter = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium' })

// Every MCP client that registered itself, for admins, with Delete. Deleting one disconnects it
// for everyone; it can register again.
export function RegisteredClients() {
  const { t } = useTranslation('oauth')
  const confirm = useConfirm()
  const { data, error, mutate } = useSWR('v1/oauth/clients', listOAuthClients)

  function remove(client: OAuthClient) {
    const name = client.name || t('connectedApps.unnamed')
    confirm({
      title: t('connectedApps.clients.confirmTitle', { name }),
      message: t('connectedApps.clients.confirmMessage'),
      confirmText: t('connectedApps.clients.delete'),
      tone: 'danger',
      onConfirm: async () => {
        try {
          await deleteOAuthClient(client.id)
          toast.success(t('connectedApps.clients.deleted', { name }))
          await mutate()
        }
        catch (failure) {
          console.debug('RegisteredClients could not delete a client', { failure })
          toast.danger(t('connectedApps.clients.loadError'))
          throw failure
        }
      },
    })
  }

  return <SecuritySection
    title={t('connectedApps.clients.title')}
    description={t('connectedApps.clients.description')}
  >
    <div className='mt-4'>
      {error && <p className='text-sm text-danger'>{t('connectedApps.clients.loadError')}</p>}
      {!error && !data && <Spinner size='sm' />}
      {data && !data.clients.length && <p className='text-sm opacity-70'>{
        t('connectedApps.clients.empty')
      }</p>}
      <ul className='flex flex-col gap-2'>{
        data?.clients.map((client) => {
          const host = clientHost(client)
          return <li key={client.id} className='level gap-3 rounded-lg bg-surface-secondary px-4 py-3'>
            <div className='level-left min-w-0 gap-3'>
              <LuBlocks className='size-5 shrink-0 opacity-70' aria-hidden />
              <div className='min-w-0'>
                <p className='truncate text-sm font-medium'>{client.name || t('connectedApps.unnamed')}</p>
                <p className='truncate text-xs opacity-60'>{
                  [
                    host,
                    client.createdAt && t('connectedApps.clients.registeredOn', {
                      date: dateFormatter.format(new Date(client.createdAt)),
                    }),
                  ].filter(Boolean).join(' · ')
                }</p>
                <p className='truncate font-mono text-xs opacity-40'>{client.id}</p>
              </div>
            </div>
            <Button size='sm' variant='ghost' onPress={() => remove(client)}>
              <span>{t('connectedApps.clients.delete')}</span>
            </Button>
          </li>
        })
      }</ul>
    </div>
  </SecuritySection>
}
