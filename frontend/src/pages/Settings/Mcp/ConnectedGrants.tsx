// Copyright © 2026 Jalapeno Labs

import type { OAuthGrant } from '../../../api/routes/oauthRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Chip, Spinner, toast } from '@heroui/react'
import { LuPlug } from 'react-icons/lu'

// Misc
import { formatDate } from '../../../format/dates'
import { revokeOAuthGrant } from '../../../api/routes/oauthRoutes'
import { useConfirm } from '../../../hooks/useConfirm'
import { knownScopes, scopeLabelKeys } from '../../OAuth/oauthPresentation'

type Props = {
  // Undefined until the list has loaded.
  grants: OAuthGrant[] | undefined
  hasFailed: boolean
  // Fetches the list again after a client was disconnected.
  onChanged: () => Promise<unknown>
}

// The MCP clients the signed-in person connected, each with what it may do, and Disconnect.
export function ConnectedGrants(props: Props) {
  const { t } = useTranslation('oauth')
  const confirm = useConfirm()

  function disconnect(grant: OAuthGrant) {
    const name = grant.client.name || t('mcp.unnamed')
    confirm({
      title: t('mcp.grants.confirmTitle', { name }),
      message: t('mcp.grants.confirmMessage'),
      confirmText: t('mcp.grants.disconnect'),
      tone: 'danger',
      onConfirm: async () => {
        try {
          await revokeOAuthGrant(grant.client.id)
          toast.success(t('mcp.grants.disconnected', { name }))
          await props.onChanged()
        }
        catch (failure) {
          console.debug('ConnectedGrants could not disconnect a client', { failure })
          toast.danger(t('mcp.grants.disconnectError', { name }))
          throw failure
        }
      },
    })
  }

  if (props.hasFailed) {
    return <p className='text-sm text-danger'>{t('mcp.grants.loadError')}</p>
  }
  if (!props.grants) {
    return <Spinner size='sm' />
  }
  if (!props.grants.length) {
    return <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
      t('mcp.grants.empty')
    }</p>
  }

  return <ul className='flex flex-col gap-2'>{
    props.grants.map((grant) => <li
      key={grant.client.id}
      className='level gap-3 rounded-lg bg-surface-secondary px-4 py-3'
    >
      <div className='level-left min-w-0 gap-3'>
        <LuPlug className='size-5 shrink-0 text-accent' aria-hidden />
        <div className='min-w-0'>
          <p className='truncate text-sm font-medium'>{
            grant.client.name || t('mcp.unnamed')
          }</p>
          <div className='mt-1 flex flex-wrap gap-1'>{
            knownScopes(grant.scopes).map((scope) => <Chip key={scope} size='sm' variant='soft'>{
              t(scopeLabelKeys[scope])
            }</Chip>)
          }</div>
          {grant.grantedAt && <p className='mt-1 text-xs opacity-60'>{
            t('mcp.grants.connectedOn', {
              date: formatDate(grant.grantedAt),
            })
          }</p>}
        </div>
      </div>
      <Button size='sm' variant='ghost' onPress={() => disconnect(grant)}>
        <span>{t('mcp.grants.disconnect')}</span>
      </Button>
    </li>)
  }</ul>
}
