// Copyright © 2026 Jalapeno Labs

import type { DataTableSelectionAction } from '@jalapenolabs/uikit'
import type { Initiative, InitiativeBulkChange, InitiativeRefusal } from '../../api/routes/initiativeRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { initiativesUpserted } from '../../store/initiativesSlice'

// User interface
import { LuCircleDot, LuTrash2, LuUndo2 } from 'react-icons/lu'

// Misc
import { changeInitiatives, INITIATIVE_STATES } from '../../api/routes/initiativeRoutes'
import { INITIATIVE_BULK_MAX_INITIATIVES } from '../../constants'
import { toast } from '../../toast/toast'
import { initiativeStateLabelKeys } from './initiativePresentation'

// What a selection of initiatives can have done to it all at once, for the floating bar of
// the Initiatives table: move to a state, and delete with Undo; and, for deleted initiatives,
// restore. Each sends one change through the bulk route, files the initiatives it answers in
// Redux at once (the events that follow are idempotent), and says what happened. One the
// change did not apply to is named in a warning that stays until dismissed.
export function useInitiativeBulkActions() {
  const { t } = useTranslation([ 'initiatives', 'common' ])
  const dispatch = useAppDispatch()

  // Resolves to the initiatives changed, or null when a request failed; either way the
  // person has been told.
  async function send(rows: Initiative[], change: InitiativeBulkChange) {
    const namesById = new Map(rows.map((row) => [ row.id, row.name ]))
    const initiativeIds = [ ...namesById.keys() ]
    const changed: Initiative[] = []
    const refused: InitiativeRefusal[] = []
    try {
      for (let start = 0; start < initiativeIds.length; start += INITIATIVE_BULK_MAX_INITIATIVES) {
        const part = initiativeIds.slice(start, start + INITIATIVE_BULK_MAX_INITIATIVES)
        const response = await changeInitiatives(part, change)
        dispatch(initiativesUpserted(response.initiatives))
        changed.push(...response.initiatives)
        refused.push(...response.refused)
      }
    }
    catch (error) {
      console.debug('useInitiativeBulkActions failed to change a selection', { error, change })
      toast.danger(t('common:errors.unexpected'))
      return null
    }

    if (refused.length) {
      // Usually an initiative changed elsewhere since the list drew it, such as in another tab.
      console.debug('useInitiativeBulkActions: initiatives refused the change', { refused, change })
      const [ first ] = refused
      const name = namesById.get(first.initiativeId) ?? first.initiativeId
      const othersCount = refused.length - 1
      const message = othersCount
        ? t('bulk.toasts.refusedMore', { name, count: othersCount })
        : t('bulk.toasts.refusedOne', { name })
      toast.warning(message)
    }
    return changed
  }

  async function restore(rows: Initiative[]) {
    const changed = await send(rows, { kind: 'restore' })
    if (changed?.length) {
      toast.success(t('bulk.toasts.restored', { count: changed.length }))
    }
  }

  const stateAction: DataTableSelectionAction<Initiative> = {
    id: 'state',
    label: t('bulk.state'),
    icon: <LuCircleDot className='size-4' aria-hidden />,
    choices: INITIATIVE_STATES.map((state) => ({ value: state, label: t(initiativeStateLabelKeys[state]) })),
    onChoose: async (value, rows) => {
      const state = INITIATIVE_STATES.find((candidate) => candidate === value)
      if (!state) {
        console.debug('The bulk initiative state action received a state it does not offer', { value })
        return
      }
      const changed = await send(rows, { kind: 'state', state })
      if (changed?.length) {
        toast.success(t('bulk.toasts.stateChanged', {
          count: changed.length,
          state: t(initiativeStateLabelKeys[state]),
        }))
      }
    },
  }

  // Deleting is soft, so it happens at once, with Undo restoring exactly what it deleted.
  const deleteAction: DataTableSelectionAction<Initiative> = {
    id: 'delete',
    label: t('common:actions.delete'),
    icon: <LuTrash2 className='size-4' aria-hidden />,
    isDanger: true,
    onPress: async (rows) => {
      const deleted = await send(rows, { kind: 'delete' })
      if (!deleted?.length) {
        return
      }
      toast.success(t('bulk.toasts.deleted', { count: deleted.length }), {
        action: {
          label: t('common:actions.undo'),
          onPress: () => void restore(deleted),
        },
      })
    },
  }

  const restoreAction: DataTableSelectionAction<Initiative> = {
    id: 'restore',
    label: t('list.restore'),
    icon: <LuUndo2 className='size-4' aria-hidden />,
    onPress: restore,
  }

  const liveActions: DataTableSelectionAction<Initiative>[] = [ stateAction, deleteAction ]
  const deletedActions: DataTableSelectionAction<Initiative>[] = [ restoreAction ]

  return {
    liveActions,
    deletedActions,
  } as const
}
