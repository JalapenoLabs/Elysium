// Copyright © 2026 Jalapeno Labs

import type { DataTableAction } from '@jalapenolabs/uikit'
import type { PersonRole, User } from '../../../api/routes/userRoutes'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Chip } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { LuKeyRound, LuLogOut, LuShieldOff, LuUserCheck, LuUserX } from 'react-icons/lu'
import { OptionSelect } from '../../../components/OptionSelect'

// Misc
import { DateText } from '../../../format/DateText'
import { formatDate } from '../../../format/dates'
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
  // Roles picked on the page and not yet saved, by user id. The page's Save keeps them.
  roleChanges: Record<string, PersonRole>
  onChangeRole: (user: User, role: PersonRole) => void
  onSetDisabled: (user: User, disabled: boolean) => void
  onRevokeSessions: (user: User) => void
  onResetMfa: (user: User) => void
  onCreateRecoveryLink: (user: User) => void
}

const USER_COLUMN_KEYS = [ 'name', 'role', 'status', 'signIn', 'lastSeen', 'created' ] as const
type UserColumnKey = typeof USER_COLUMN_KEYS[number]

const columnLabelKeys = {
  name: 'table.name',
  role: 'table.role',
  status: 'table.status',
  signIn: 'table.signIn',
  lastSeen: 'table.lastSeen',
  created: 'table.created',
} as const satisfies Record<UserColumnKey, string>

// Starting widths in pixels, summing to less than the settings content column.
const columnSizes = {
  name: 260,
  role: 170,
  status: 120,
  signIn: 220,
  lastSeen: 150,
  created: 140,
} as const satisfies Record<UserColumnKey, number>

// Everyone approved: their role (changed in place, kept by the page's Save), whether they may sign in, how they sign
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
        ? formatDate(user.lastSeenAt)
        : t('table.never')
    }

    const renderCell = {
      name: (user: User) => <div className='min-w-0'>
        <div className='font-medium'>{user.name}</div>
        <div className='text-xs opacity-70'>{user.email}</div>
      </div>,
      role: (user: User) => <OptionSelect
        isLabelHidden
        label={t('table.roleFor', { name: user.name })}
        options={roleOptions}
        value={props.roleChanges[user.id] ?? user.role ?? 'member'}
        onChange={(value) => {
          const role = PERSON_ROLES.find((option) => option === value)
          if (!role) {
            console.debug('UserTable ignored an unknown role', { value })
            return
          }
          props.onChangeRole(user, role)
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
      lastSeen: (user: User) => user.lastSeenAt
        ? <DateText value={user.lastSeenAt} isFocusable={false} />
        : t('table.never'),
      created: (user: User) => <DateText value={user.createdAt} isFocusable={false} />,
    } satisfies Record<UserColumnKey, (user: User) => unknown>

    // Search matches what the admin sees.
    const searchValue = {
      name: (user: User) => `${user.name} ${user.email ?? ''}`,
      role: roleText,
      status: statusText,
      signIn: methodsText,
      lastSeen: lastSeenText,
      created: (user: User) => formatDate(user.createdAt),
    } satisfies Record<UserColumnKey, (user: User) => string>

    // Sorting follows the text, except the dates, which sort in time with the never seen first.
    const sortValue = {
      ...searchValue,
      lastSeen: (user: User) => user.lastSeenAt
        ? Date.parse(user.lastSeenAt)
        : Number.NEGATIVE_INFINITY,
      created: (user: User) => Date.parse(user.createdAt),
    } satisfies Record<UserColumnKey, (user: User) => string | number>

    return createManagedColumns<User, UserColumnKey>({
      columnKeys: USER_COLUMN_KEYS,
      getColumnLabel: (columnKey) => t(columnLabelKeys[columnKey]),
      getSearchKey: (columnKey) => columnKey,
      getSearchValue: (columnKey) => searchValue[columnKey],
      createColumnDef: ({ columnKey, columnId, columnLabel }) => ({
        id: columnId,
        header: columnLabel,
        accessorFn: sortValue[columnKey],
        size: columnSizes[columnKey],
        cell: ({ row }) => renderCell[columnKey](row.original),
      }),
    })
  }, [ t, props.roleChanges, props.onChangeRole ])

  // The two used most stay buttons; resetting the authenticator and disabling sit in the menu,
  // the dangerous one last.
  const rowActions = useMemo((): DataTableAction<User>[] => [
    {
      id: 'recovery',
      label: t('actions.recoveryLink'),
      icon: <LuKeyRound className='size-4' aria-hidden />,
      onPress: (row) => props.onCreateRecoveryLink(row.original),
    },
    {
      id: 'revoke',
      label: t('actions.revokeSessions'),
      icon: <LuLogOut className='size-4' aria-hidden />,
      onPress: (row) => props.onRevokeSessions(row.original),
    },
    {
      id: 'resetMfa',
      label: t('actions.resetMfa'),
      icon: <LuShieldOff className='size-4' aria-hidden />,
      onPress: (row) => props.onResetMfa(row.original),
    },
    {
      id: 'enable',
      label: t('actions.enable'),
      icon: <LuUserCheck className='size-4' aria-hidden />,
      isVisible: (row) => row.original.status === 'disabled',
      onPress: (row) => props.onSetDisabled(row.original, false),
    },
    {
      id: 'disable',
      label: t('actions.disable'),
      icon: <LuUserX className='size-4' aria-hidden />,
      isDanger: true,
      isVisible: (row) => row.original.status !== 'disabled',
      onPress: (row) => props.onSetDisabled(row.original, true),
    },
  ], [ t, props.onCreateRecoveryLink, props.onRevokeSessions, props.onResetMfa, props.onSetDisabled ])

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
    rowActions={rowActions}
    pagination={{ show: true }}
    enableSorting
    stickyHeader={false}
  />
}
