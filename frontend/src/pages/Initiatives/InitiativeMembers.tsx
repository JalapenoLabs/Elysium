// Copyright © 2026 Jalapeno Labs

import type { DataTableAction } from '@jalapenolabs/uikit'
import type { ActionItem } from '../../api/routes/actionItemRoutes'
import type { Initiative } from '../../api/routes/initiativeRoutes'
import type { ItemColumnKey } from '../ActionItems/ActionItemTable'

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../../store/hooks'
import { actionItemUpserted, selectInitiativeActionItems } from '../../store/actionItemsSlice'

// User interface
import { buttonVariants, Link, Spinner } from '@heroui/react'
import { LuPlus, LuX } from 'react-icons/lu'
import { EmptyNotice } from '../../components/EmptyNotice'
import { ActionItemTable } from '../ActionItems/ActionItemTable'
import { AddItemToInitiative } from './AddItemToInitiative'

// Misc
import { joinInitiative, leaveInitiative } from '../../api/routes/actionItemRoutes'
import { useActionItemsLoader } from '../../hooks/useServerData'
import { useNow } from '../../hooks/useNow'
import { toast } from '../../toast/toast'
import { getNewActionItemUrl } from '../../urls'

type Props = {
  initiative: Initiative
}

// Every member is in this initiative, and the page shows when each changed in its history.
const OMITTED_COLUMNS: readonly ItemColumnKey[] = [ 'initiatives', 'updated' ]

// The items an initiative tracks, whatever their state, with ways to add an existing item,
// start a new one in the initiative, or take one out.
export function InitiativeMembers(props: Props) {
  const { t } = useTranslation([ 'initiatives', 'common' ])
  const dispatch = useAppDispatch()
  const status = useActionItemsLoader()
  const now = useNow()
  const initiativeId = props.initiative.id
  const members = useAppSelector((state) => selectInitiativeActionItems(state, initiativeId))
  const isDeleted = Boolean(props.initiative.deletedAt)

  // Taking an item out leaves the item itself, and its time in the initiative stays in the
  // burnup's history. Joining again puts it back, so it happens at once, with Undo in its toast.
  const memberRowActions = useMemo((): DataTableAction<ActionItem>[] => {
    // The row is gone by now, so this reports through toasts alone.
    async function putBack(item: ActionItem) {
      try {
        const response = await joinInitiative(item.id, initiativeId)
        dispatch(actionItemUpserted(response.item))
        toast.success(t('toasts.itemPutBack', { title: item.title }))
      }
      catch (error) {
        console.debug('InitiativeMembers failed to put an item back', { error, itemId: item.id })
        toast.danger(t('common:errors.unexpected'))
      }
    }

    async function leave(item: ActionItem) {
      try {
        const response = await leaveInitiative(item.id, initiativeId)
        dispatch(actionItemUpserted(response.item))
        toast.success(t('toasts.itemTakenOut', { title: item.title }), {
          action: {
            label: t('common:actions.undo'),
            onPress: () => void putBack(item),
          },
        })
      }
      catch (error) {
        console.debug('InitiativeMembers failed to take an item out', { error, itemId: item.id })
        toast.danger(t('common:errors.unexpected'))
      }
    }

    return [{
      id: 'leave',
      label: t('members.remove'),
      icon: <LuX className='size-4' aria-hidden />,
      onPress: (row) => leave(row.original),
    }]
  }, [ t, dispatch, initiativeId ])

  return <div>
    {!isDeleted && <div className='compact flex flex-wrap items-end gap-3'>
      <AddItemToInitiative initiative={props.initiative} />
      <Link
        href={getNewActionItemUrl({ initiativeId })}
        className={buttonVariants({ size: 'md', variant: 'outline', className: 'gap-2' })}
      >
        <LuPlus className='size-4' aria-hidden />
        <span>{t('members.new')}</span>
      </Link>
    </div>}

    {status === 'loading' && <div className='grid place-items-center py-10'>
      <Spinner />
    </div>}

    {status !== 'loading' && !members.length && <EmptyNotice>{
      isDeleted
        ? t('members.hiddenWhileDeleted')
        : t('members.empty')
    }</EmptyNotice>}

    {members.length > 0 && <ActionItemTable
      items={members}
      now={now}
      label={t('members.heading')}
      storageId='elysium.initiatives.members.table.v1'
      omittedColumns={OMITTED_COLUMNS}
      rowActions={isDeleted
        ? undefined
        : memberRowActions}
    />}
  </div>
}
