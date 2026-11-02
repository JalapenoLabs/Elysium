// Copyright © 2026 Jalapeno Labs

import type { Account } from '../../../api/routes/connectorRoutes'

// Core
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { Button, Card, Chip, Link, Spinner, toast } from '@heroui/react'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { deleteTokenAccount, listAccounts } from '../../../api/routes/connectorRoutes'
import { useConfirm } from '../../../hooks/useConfirm'
import { getBrowserSessionUrl, getGithubCredentialEditUrl, getJiraCredentialEditUrl } from '../../../urls'

// Where each kind of account is edited: its own settings page, or nowhere for a key, which is
// replaced by removing it and adding another.
const editUrlByKind = {
  atlassian: getJiraCredentialEditUrl,
  github: getGithubCredentialEditUrl,
  browser: getBrowserSessionUrl,
  fireflies: null,
} as const satisfies Record<Account['kind'], ((id: string) => string) | null>

// Every account Elysium signs in with, and what each one feeds. Reloaded by the event stream
// when a connector or account changes.
export function AccountsList() {
  const { t } = useTranslation([ 'connectors', 'common' ])
  const confirm = useConfirm()
  const { data, error, mutate } = useSWR('v1/accounts', listAccounts)

  function remove(account: Account) {
    confirm({
      title: t('accounts.deleteTitle'),
      message: t('accounts.deleteBody'),
      tone: 'danger',
      confirmText: t('accounts.delete'),
      onConfirm: async () => {
        try {
          await deleteTokenAccount(account.id)
          await mutate()
          toast.success(t('accounts.deleted'))
        }
        catch (deleteError) {
          console.debug('AccountsList failed to remove an account', { deleteError, accountId: account.id })
          toast.danger(getApiErrorMessage(deleteError) ?? t('common:errors.unexpected'))
        }
      },
    })
  }

  if (error) {
    return <p className='text-sm text-danger'>{t('common:errors.unexpected')}</p>
  }
  if (!data) {
    return <div className='grid place-items-center py-10'><Spinner /></div>
  }
  if (!data.accounts.length) {
    return <p className='text-sm opacity-70'>{t('accounts.empty')}</p>
  }

  return <div>{
    data.accounts.map((account) => {
      const editUrl = editUrlByKind[account.kind]
      return <Card key={`${account.kind}-${account.id}`} className='compact p-4'>
        <div className='level items-start gap-4'>
          <div className='min-w-0'>
            <div className='flex flex-wrap items-center gap-2'>
              <span className='font-semibold'>{account.name}</span>
              <Chip size='sm' variant='soft'>{t(`accountKinds.${account.kind}`)}</Chip>
              {account.isDefault && <Chip size='sm' variant='soft' color='accent'>{t('accounts.default')}</Chip>}
            </div>
            {account.identity && <p className='text-sm opacity-70 break-words'>{account.identity}</p>}
            <p className='text-sm opacity-60'>{
              account.usedBy.length
                ? t('accounts.usedBy', { count: account.usedBy.length })
                : t('accounts.unused')
            }</p>
          </div>
          <div className='flex shrink-0 gap-2'>
            {editUrl && <Link href={editUrl(account.id)} className='text-sm'>{t('accounts.edit')}</Link>}
            {account.kind === 'fireflies' && <Button size='sm' variant='outline' onPress={() => remove(account)}>
              <span>{t('accounts.delete')}</span>
            </Button>}
          </div>
        </div>
      </Card>
    })
  }</div>
}
