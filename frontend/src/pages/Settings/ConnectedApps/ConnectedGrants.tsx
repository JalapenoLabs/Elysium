// Copyright © 2026 Jalapeno Labs

import type { OAuthGrant } from '../../../api/routes/oauthRoutes'

// Core
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { Button, Chip, Spinner, toast } from '@heroui/react'
import { LuPlug } from 'react-icons/lu'
import { SecuritySection } from '../Security/SecuritySection'

// Misc
import { listOAuthGrants, revokeOAuthGrant } from '../../../api/routes/oauthRoutes'
import { useConfirm } from '../../../hooks/useConfirm'
import { knownScopes, scopeLabelKeys } from '../../OAuth/oauthPresentation'

const dateFormatter = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium' })

// The MCP clients the signed-in person connected, each with what it may do, and Disconnect.
// Nothing streams for grants, so the list is fetched again after a change.
export function ConnectedGrants() {
  const { t } = useTranslation('oauth')
  const confirm = useConfirm()
  const { data, error, mutate } = useSWR('v1/oauth/grants', listOAuthGrants)

  function disconnect(grant: OAuthGrant) {
    const name = grant.client.name || t('connectedApps.unnamed')
    confirm({
      title: t('connectedApps.grants.confirmTitle', { name }),
      message: t('connectedApps.grants.confirmMessage'),
      confirmText: t('connectedApps.grants.disconnect'),
      tone: 'danger',
      onConfirm: async () => {
        try {
          await revokeOAuthGrant(grant.client.id)
          toast.success(t('connectedApps.grants.disconnected', { name }))
          await mutate()
        }
        catch (failure) {
          console.debug('ConnectedGrants could not disconnect a client', { failure })
          toast.danger(t('connectedApps.grants.loadError'))
          throw failure
        }
      },
    })
  }

  return <SecuritySection
    title={t('connectedApps.grants.title')}
    description={t('connectedApps.grants.description')}
  >
    <div className='mt-4'>
      {error && <p className='text-sm text-danger'>{t('connectedApps.grants.loadError')}</p>}
      {!error && !data && <Spinner size='sm' />}
      {data && !data.grants.length && <p className='text-sm opacity-70'>{
        t('connectedApps.grants.empty')
      }</p>}
      <ul className='flex flex-col gap-2'>{
        data?.grants.map((grant) => <li
          key={grant.client.id}
          className='level gap-3 rounded-lg bg-surface-secondary px-4 py-3'
        >
          <div className='level-left min-w-0 gap-3'>
            <LuPlug className='size-5 shrink-0 text-accent' aria-hidden />
            <div className='min-w-0'>
              <p className='truncate text-sm font-medium'>{
                grant.client.name || t('connectedApps.unnamed')
              }</p>
              <div className='mt-1 flex flex-wrap gap-1'>{
                knownScopes(grant.scopes).map((scope) => <Chip key={scope} size='sm' variant='soft'>{
                  t(scopeLabelKeys[scope])
                }</Chip>)
              }</div>
              {grant.grantedAt && <p className='mt-1 text-xs opacity-60'>{
                t('connectedApps.grants.connectedOn', {
                  date: dateFormatter.format(new Date(grant.grantedAt)),
                })
              }</p>}
            </div>
          </div>
          <Button size='sm' variant='ghost' onPress={() => disconnect(grant)}>
            <span>{t('connectedApps.grants.disconnect')}</span>
          </Button>
        </li>)
      }</ul>
    </div>
  </SecuritySection>
}
