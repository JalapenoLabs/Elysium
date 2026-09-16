// Copyright © 2026 Jalapeno Labs

import type { GithubCredential } from '../../../api/routes/githubRoutes'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Chip } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { GithubCredentialRowActions } from './GithubCredentialRowActions'

// Misc
import { useSmartTableLabels } from '../../../hooks/useSmartTableLabels'
import { githubKindLabelKeys } from './githubPresentation'

type Props = {
  credentials: GithubCredential[]
  onEdit: (credential: GithubCredential) => void
  onTest: (credential: GithubCredential) => void
  onToggleDefault: (credential: GithubCredential) => void
  onDelete: (credential: GithubCredential) => void
}

const GITHUB_COLUMN_KEYS = [ 'name', 'kind', 'account', 'scopes', 'expires', 'rowActions' ] as const
type GithubColumnKey = typeof GITHUB_COLUMN_KEYS[number]

const columnLabelKeys = {
  name: 'table.name',
  kind: 'table.kind',
  account: 'table.account',
  scopes: 'table.scopes',
  expires: 'table.expires',
  rowActions: 'common:actions.moreActions',
} as const satisfies Record<GithubColumnKey, string>

// Starting widths in pixels, summing to less than the settings content column.
const columnSizes = {
  name: 200,
  kind: 130,
  account: 160,
  scopes: 260,
  expires: 150,
  rowActions: 64,
} as const satisfies Record<GithubColumnKey, number>

export function GithubCredentialTable(props: Props) {
  const { t, i18n } = useTranslation([ 'github', 'common' ])
  const labels = useSmartTableLabels()
  const [ search, setSearch ] = useState('')

  // Captured once per mount so render stays pure; expiry is "as of when you opened the page".
  const [ now ] = useState(() => Date.now())

  const managedColumns = useMemo(() => {
    // Timestamps arrive as UTC; this is the one place they become local time. A token
    // expires on a date, so the time of day would only be noise.
    const dateFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium' })

    // A fine-grained token's permissions are per repository, and GitHub does not report
    // them, so it has no scopes to show.
    function scopesText(credential: GithubCredential) {
      if (credential.kind === 'fine-grained') {
        return t('table.perRepository')
      }
      if (!credential.scopes.length) {
        return t('table.noScopes')
      }
      return credential.scopes.join(', ')
    }

    function expiresText(credential: GithubCredential) {
      if (!credential.tokenExpiresAt) {
        return t('table.never')
      }
      return dateFormatter.format(new Date(credential.tokenExpiresAt))
    }

    const renderCell = {
      name: (credential: GithubCredential) => <div className='flex items-center gap-2'>
        <span className='font-medium'>{credential.name}</span>
        {credential.isDefault && <Chip size='sm' variant='soft' color='accent'>{t('table.default')}</Chip>}
      </div>,
      kind: (credential: GithubCredential) => t(githubKindLabelKeys[credential.kind]),
      account: (credential: GithubCredential) => <code className='text-xs'>{credential.login}</code>,
      scopes: (credential: GithubCredential) => <span className='line-clamp-2'>{
        scopesText(credential)
      }</span>,
      expires: (credential: GithubCredential) => {
        const hasExpired = credential.tokenExpiresAt !== null
          && Date.parse(credential.tokenExpiresAt) <= now
        if (hasExpired) {
          return <Chip size='sm' variant='soft' color='danger'>{t('table.expired')}</Chip>
        }
        return expiresText(credential)
      },
      rowActions: (credential: GithubCredential) => <GithubCredentialRowActions
        credential={credential}
        onEdit={props.onEdit}
        onTest={props.onTest}
        onToggleDefault={props.onToggleDefault}
        onDelete={props.onDelete}
      />,
    } satisfies Record<GithubColumnKey, (credential: GithubCredential) => unknown>

    // Search matches what the user sees, not raw enum values or timestamps.
    const searchValue = {
      name: (credential: GithubCredential) => credential.name,
      kind: (credential: GithubCredential) => t(githubKindLabelKeys[credential.kind]),
      account: (credential: GithubCredential) => credential.login,
      scopes: scopesText,
      expires: expiresText,
      rowActions: null,
    } satisfies Record<GithubColumnKey, ((credential: GithubCredential) => string) | null>

    return createManagedColumns<GithubCredential, GithubColumnKey>({
      columnKeys: GITHUB_COLUMN_KEYS,
      getColumnLabel: (columnKey) => t(columnLabelKeys[columnKey]),
      getSearchKey: (columnKey) => searchValue[columnKey]
        ? columnKey
        : null,
      getSearchValue: (columnKey) => searchValue[columnKey],
      createColumnDef: ({ columnKey, columnId, columnLabel }) => {
        const toSearchText = searchValue[columnKey]
        if (!toSearchText) {
          return {
            id: columnId,
            header: () => <span className='sr-only'>{columnLabel}</span>,
            enableSorting: false,
            size: columnSizes[columnKey],
            cell: ({ row }) => renderCell[columnKey](row.original),
          }
        }

        // Expiry sorts by date, not by its formatted text, with no expiry as the furthest away.
        if (columnKey === 'expires') {
          return {
            id: columnId,
            header: columnLabel,
            accessorFn: (credential) => credential.tokenExpiresAt
              ? Date.parse(credential.tokenExpiresAt)
              : Number.POSITIVE_INFINITY,
            size: columnSizes[columnKey],
            cell: ({ row }) => renderCell[columnKey](row.original),
          }
        }

        return {
          id: columnId,
          header: columnLabel,
          accessorFn: toSearchText,
          size: columnSizes[columnKey],
          cell: ({ row }) => renderCell[columnKey](row.original),
        }
      },
    })
  }, [ t, i18n.language, now, props.onEdit, props.onTest, props.onToggleDefault, props.onDelete ])

  if (!props.credentials.length) {
    return <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
      t('table.empty')
    }</p>
  }

  return <SmartTable
    ids={{
      tableElementId: 'github-credentials-table',
      tableLocalStorageId: 'elysium.settings.github.table',
    }}
    tableAriaLabel={t('table.label')}
    data={props.credentials}
    managedColumns={managedColumns}
    getRowId={(credential) => credential.id}
    labels={labels}
    search={{
      value: search,
      onChange: setSearch,
    }}
    enableSorting
    stickyHeader={false}
  />
}
