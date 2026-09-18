// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type { JiraCredential } from '../../../api/routes/jiraRoutes'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Chip } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { JiraCredentialRowActions } from './JiraCredentialRowActions'

// Misc
import { ALL_JIRA_ITEMS } from '../../../api/routes/jiraRoutes'
import { useSmartTableLabels } from '../../../hooks/useSmartTableLabels'
import { summarizeScope } from './jiraPresentation'

type Props = {
  credentials: JiraCredential[]
  onEdit: (credential: JiraCredential) => void
  onTest: (credential: JiraCredential) => void
  onDelete: (credential: JiraCredential) => void
}

const JIRA_COLUMN_KEYS = [
  'name',
  'site',
  'account',
  'projects',
  'boards',
  'checked',
  'rowActions',
] as const
type JiraColumnKey = typeof JIRA_COLUMN_KEYS[number]

const columnLabelKeys = {
  name: 'table.name',
  site: 'table.site',
  account: 'table.account',
  projects: 'table.projects',
  boards: 'table.boards',
  checked: 'table.checked',
  rowActions: 'common:actions.moreActions',
} as const satisfies Record<JiraColumnKey, string>

// Starting widths in pixels, summing to less than the settings content column.
const columnSizes = {
  name: 180,
  site: 180,
  account: 190,
  projects: 190,
  boards: 190,
  checked: 170,
  rowActions: 64,
} as const satisfies Record<JiraColumnKey, number>

export function JiraCredentialTable(props: Props) {
  const { t, i18n } = useTranslation([ 'jira', 'common' ])
  const labels = useSmartTableLabels()
  const [ search, setSearch ] = useState('')

  const managedColumns = useMemo(() => {
    // Timestamps arrive as UTC; this is the one place they become local time.
    const dateFormatter = new Intl.DateTimeFormat(i18n.language, {
      dateStyle: 'medium',
      timeStyle: 'short',
    })

    // Searching an allowlist matches every name in it, not only the few a cell shows.
    // `names` is null for an allowlist of everything, which names nothing in particular.
    function scopeText(names: string[] | null) {
      if (!names) {
        return t('table.all')
      }
      if (!names.length) {
        return t('table.none')
      }
      return names.join(', ')
    }

    function scopeCell(names: string[] | null, countLabel: string): ReactNode {
      if (!names) {
        return <Chip size='sm' variant='soft' color='accent'>{t('table.all')}</Chip>
      }
      if (!names.length) {
        return <span className='opacity-70'>{t('table.none')}</span>
      }

      const summary = summarizeScope(names)
      return <div className='flex flex-col'>
        <span>{countLabel}</span>
        <span className='truncate text-xs opacity-70'>{
          summary.remaining
            ? t('table.andMore', { count: summary.remaining, names: summary.preview.join(', ') })
            : summary.preview.join(', ')
        }</span>
      </div>
    }

    const renderCell = {
      name: (credential: JiraCredential) => <span className='font-medium'>{credential.name}</span>,
      // The scheme is the same on every Jira Cloud site, so only the host is worth room.
      site: (credential: JiraCredential) => <code className='text-xs'>{
        credential.siteUrl.replace('https://', '')
      }</code>,
      account: (credential: JiraCredential) => <div className='flex flex-col'>
        <span>{credential.displayName}</span>
        <span className='truncate text-xs opacity-70'>{credential.accountEmail}</span>
      </div>,
      projects: (credential: JiraCredential) => scopeCell(
        credential.projects === ALL_JIRA_ITEMS
          ? null
          : credential.projects.map((project) => project.key),
        t('table.projectCount', {
          count: credential.projects === ALL_JIRA_ITEMS
            ? 0
            : credential.projects.length,
        }),
      ),
      boards: (credential: JiraCredential) => scopeCell(
        credential.boards === ALL_JIRA_ITEMS
          ? null
          : credential.boards.map((board) => board.name),
        t('table.boardCount', {
          count: credential.boards === ALL_JIRA_ITEMS
            ? 0
            : credential.boards.length,
        }),
      ),
      checked: (credential: JiraCredential) => dateFormatter.format(new Date(credential.checkedAt)),
      rowActions: (credential: JiraCredential) => <JiraCredentialRowActions
        credential={credential}
        onEdit={props.onEdit}
        onTest={props.onTest}
        onDelete={props.onDelete}
      />,
    } satisfies Record<JiraColumnKey, (credential: JiraCredential) => ReactNode>

    // Search matches what the user sees, not raw ids or timestamps.
    const searchValue = {
      name: (credential: JiraCredential) => credential.name,
      site: (credential: JiraCredential) => credential.siteUrl,
      account: (credential: JiraCredential) => `${credential.displayName} ${credential.accountEmail}`,
      projects: (credential: JiraCredential) => scopeText(
        credential.projects === ALL_JIRA_ITEMS
          ? null
          : credential.projects.map((project) => `${project.key} ${project.name}`),
      ),
      boards: (credential: JiraCredential) => scopeText(
        credential.boards === ALL_JIRA_ITEMS
          ? null
          : credential.boards.map((board) => board.name),
      ),
      checked: (credential: JiraCredential) => dateFormatter.format(new Date(credential.checkedAt)),
      rowActions: null,
    } satisfies Record<JiraColumnKey, ((credential: JiraCredential) => string) | null>

    return createManagedColumns<JiraCredential, JiraColumnKey>({
      columnKeys: JIRA_COLUMN_KEYS,
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

        // The last check sorts by instant, not by its formatted text.
        if (columnKey === 'checked') {
          return {
            id: columnId,
            header: columnLabel,
            accessorFn: (credential) => Date.parse(credential.checkedAt),
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
  }, [ t, i18n.language, props.onEdit, props.onTest, props.onDelete ])

  if (!props.credentials.length) {
    return <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
      t('table.empty')
    }</p>
  }

  return <SmartTable
    ids={{
      tableElementId: 'jira-credentials-table',
      tableLocalStorageId: 'elysium.settings.jira.table',
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
