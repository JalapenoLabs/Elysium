// Copyright © 2026 Jalapeno Labs

import type { ActionItem } from '../../api/routes/actionItemRoutes'
import type { Initiative } from '../../api/routes/initiativeRoutes'
import type { ItemColumnKey } from '../ActionItems/ActionItemTable'

// Core
import { useCallback } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectInitiativeActionItems } from '../../store/actionItemsSlice'

// User interface
import { buttonVariants, Link, Spinner } from '@heroui/react'
import { LuPlus } from 'react-icons/lu'
import { EmptyNotice } from '../../components/EmptyNotice'
import { ActionItemTable } from '../ActionItems/ActionItemTable'
import { AddItemToInitiative } from './AddItemToInitiative'
import { LeaveInitiativeButton } from './LeaveInitiativeButton'

// Misc
import { useActionItemsLoader } from '../../hooks/useServerData'
import { useNow } from '../../hooks/useNow'
import { getNewActionItemUrl } from '../../urls'

type Props = {
  initiative: Initiative
}

// Every member is in this initiative, and the page shows when each changed in its history.
const OMITTED_COLUMNS: readonly ItemColumnKey[] = [ 'initiatives', 'updated' ]

// The items an initiative tracks, whatever their state, with ways to add an existing item,
// start a new one in the initiative, or take one out.
export function InitiativeMembers(props: Props) {
  const { t } = useTranslation('initiatives')
  const status = useActionItemsLoader()
  const now = useNow()
  const initiativeId = props.initiative.id
  const members = useAppSelector((state) => selectInitiativeActionItems(state, initiativeId))
  const isDeleted = Boolean(props.initiative.deletedAt)

  const renderLeaveButton = useCallback(
    (item: ActionItem) => <LeaveInitiativeButton
      item={item}
      initiativeId={initiativeId}
    />,
    [ initiativeId ],
  )

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
      t('members.empty')
    }</EmptyNotice>}

    {members.length > 0 && <ActionItemTable
      items={members}
      now={now}
      label={t('members.heading')}
      storageId='elysium.initiatives.members.table.v1'
      omittedColumns={OMITTED_COLUMNS}
      renderRowActions={isDeleted
        ? undefined
        : renderLeaveButton}
    />}
  </div>
}
