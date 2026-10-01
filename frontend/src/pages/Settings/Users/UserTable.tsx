// Copyright © 2026 Jalapeno Labs

import type { PersonRole, User } from '../../../api/routes/userRoutes'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Chip } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { OptionSelect } from '../../../components/OptionSelect'
import { UserRowActions } from './UserRowActions'

// Misc
import { useSmartTableLabels } from '../../../hooks/useSmartTableLabels'
import {
  describeSignInMethods,
  PERSON_ROLES,
  roleLabelKeys,
  statusChipColors,
  statusLabelKeys,
} from './usersPresentation'

type Props = {
  users: User[]
  onChangeRole: (user: User, role: PersonRole) => void
  onSetDisabled: (user: User, disabled: boolean) => void
  onRevokeSessions: (user: User) => void
  onResetMfa: (user: User) => void
  onCreateRecoveryLink: (user: User) => void
}

const USER_COLUMN_KEYS = [ 'name', 'role', 'status', 'signIn', 'lastSeen', 'created', 'rowActions' ] as const
type UserColumnKey = typeof USER_COLUMN_KEYS[number]

const columnLabelKeys = {
  name: 'table.name',
  role: 'table.role',
  status: 'table.status',
  signIn: 'table.signIn',
  lastSeen: 'table.lastSeen',
  created: 'table.created',
  rowActions: 'common:actions.moreActions',
} as const satisfies Record<UserColumnKey, string>

// Starting widths in pixels, summing to less than the settings content column.
const columnSizes = {
  name: 260,
  role: 170,
  status: 120,
  signIn: 220,
  lastSeen: 150,
  created: 140,
  rowActions: 64,
} as const satisfies Record<UserColumnKey, number>

const dateFormatter = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium' })

// Everyone approved: their role (changed in place), whether they may sign in, how they sign
// in, and when they were last seen.
export function UserTable(props: Props) {
  const { t } = useTranslation([ 'users', 'common' ])
  const labels = useSmartTableLabels()
  const [ search, setSearch ] = useState('')

  const managedColumns = useMemo(() => {
    const roleOptions = PERSON_ROLES.map((role) => ({ id: role, label: t(roleLabelKeys[role]) }))

    function roleText(user: User) {
      return user.role
        ? t(roleLabelKeys[user.role])
        : ''
    }

    function statusText(user: User) {
      return user.status
        ? t(statusLabelKeys[user.status])
        : ''
    }

    function methodsText(user: User) {
      return describeSignInMethods(user.signInMethods)
        .map((entry) => entry.key
          ? t(entry.key)
          : entry.method)
        .join(', ')
    }

    function lastSeenText(user: User) {
      return user.lastSeenAt
        ? dateFormatter.format(new Date(user.lastSeenAt))
        : t('table.never')
    }

    const renderCell = {
      name: (user: User) => <div className='min-w-0'>
        <div className='truncate font-medium'>{user.name}</div>
        <div className='truncate text-xs opacity-70'>{user.email}</div>
      </div>,
      role: (user: User) => <OptionSelect
        isLabelHidden
        label={t('table.roleFor', { name: user.name })}
        options={roleOptions}
        value={user.role ?? 'member'}
        onChange={(value) => {
          const role = PERSON_ROLES.find((option) => option === value)
          if (role && role !== user.role) {
            props.onChangeRole(user, role)
          }
        }}
      />,
      status: (user: User) => user.status && <Chip size='sm' variant='soft' color={statusChipColors[user.status]}>{
        t(statusLabelKeys[user.status])
      }</Chip>,
      signIn: (user: User) => <div className='flex flex-wrap gap-1'>{
        describeSignInMethods(user.signInMethods).map((entry) => <Chip key={entry.method} size='sm' variant='soft'>{
          entry.key
            ? t(entry.key)
            : entry.method
        }</Chip>)
      }</div>,
      lastSeen: lastSeenText,
      created: (user: User) => dateFormatter.format(new Date(user.createdAt)),
      rowActions: (user: User) => <UserRowActions
        user={user}
        onSetDisabled={props.onSetDisabled}
        onRevokeSessions={props.onRevokeSessions}
        onResetMfa={props.onResetMfa}
        onCreateRecoveryLink={props.onCreateRecoveryLink}
      />,
    } satisfies Record<UserColumnKey, (user: User) => unknown>

    // Search matches what the admin sees.
    const searchValue = {
      name: (user: User) => `${user.name} ${user.email ?? ''}`,
      role: roleText,
      status: statusText,
      signIn: methodsText,
      lastSeen: lastSeenText,
      created: (user: User) => user.createdAt,
      rowActions: null,
    } satisfies Record<UserColumnKey, ((user: User) => string) | null>

    return createManagedColumns<User, UserColumnKey>({
      columnKeys: USER_COLUMN_KEYS,
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

        return {
          id: columnId,
          header: columnLabel,
          accessorFn: toSearchText,
          size: columnSizes[columnKey],
          cell: ({ row }) => renderCell[columnKey](row.original),
        }
      },
    })
  }, [
    t,
    props.onChangeRole,
    props.onSetDisabled,
    props.onRevokeSessions,
    props.onResetMfa,
    props.onCreateRecoveryLink,
  ])

  if (!props.users.length) {
    return <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
      t('table.empty')
    }</p>
  }

  return <SmartTable
    ids={{
      tableElementId: 'users-table',
      tableLocalStorageId: 'elysium.settings.users.table',
    }}
    tableAriaLabel={t('table.label')}
    data={props.users}
    managedColumns={managedColumns}
    getRowId={(user) => user.id}
    labels={labels}
    search={{
      value: search,
      onChange: setSearch,
    }}
    enableSorting
    stickyHeader={false}
  />
}
