// Copyright © 2026 Jalapeno Labs

import type { DataTableSelectionAction } from '@jalapenolabs/uikit'
import type { ParseKeys } from 'i18next'
import type { ActionItem, ActionItemBulkChange, ActionItemRefusal } from '../../api/routes/actionItemRoutes'

// Core
import { useRef } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../../store/hooks'
import { actionItemsUpserted } from '../../store/actionItemsSlice'
import { selectIsCompanyMember } from '../../store/authSlice'
import { selectLiveInitiatives } from '../../store/initiativesSlice'

// User interface
import { useOverlayState } from '@heroui/react'
import {
  LuCalendarClock,
  LuCircleDot,
  LuFlag,
  LuFolderInput,
  LuFolderMinus,
  LuTrash2,
  LuUndo2,
  LuWorkflow,
} from 'react-icons/lu'
import { SnoozeDateModal } from './SnoozeDateModal'

// Utility
import { getLocalTimeZone } from '@internationalized/date'

// Misc
import { ACTION_ITEM_PRIORITIES, changeActionItems } from '../../api/routes/actionItemRoutes'
import { ACTION_ITEM_BULK_MAX_ITEMS } from '../../constants'
import { useInitiativesLoader } from '../../hooks/useServerData'
import { toast } from '../../toast/toast'
import { getSnoozePresetInstants, SNOOZE_PRESETS } from './actionItemDates'
import { priorityLabelKeys, snoozePresetLabelKeys, stateLabelKeys } from './actionItemPresentation'

// Where a selection's state can be moved: the inbox is never a destination.
const BULK_STATES = [ 'open', 'resolved', 'dismissed' ] as const
type BulkState = typeof BULK_STATES[number]

const stateToastKeys = {
  open: 'bulk.toasts.opened',
  resolved: 'bulk.toasts.resolved',
  dismissed: 'bulk.toasts.dismissed',
} as const satisfies Record<BulkState, ParseKeys<'actionItems'>>

// How a warning names what a change left alone: the first item by title, and how many more.
type RefusalKeys = {
  one: ParseKeys<'actionItems'>
  more: ParseKeys<'actionItems'>
}

const DEFAULT_REFUSAL_KEYS = {
  one: 'bulk.toasts.refusedOne',
  more: 'bulk.toasts.refusedMore',
} as const satisfies RefusalKeys

// An item is left unassigned for one reason in practice: no project leads it.
const UNASSIGNABLE_KEYS = {
  one: 'bulk.toasts.unassignableOne',
  more: 'bulk.toasts.unassignableMore',
} as const satisfies RefusalKeys

// The snooze choices beyond the presets.
const PICK_DAY = 'pickDay'
const END_SNOOZE = 'end'

// What a selection of items can have done to it all at once, for the floating bar of the
// All items table (`~/.claude/docs/ux.md`, Table multi-select): move to a state, set a
// priority, snooze, join or leave an initiative, assign to their Project Managers, and delete
// with Undo; and, for deleted items, restore. Each sends one change through the bulk route, in
// parts the API accepts, files the items it answers in Redux at once (the events that follow
// are idempotent), and says what happened. An item the change did not apply to is named in a
// warning that stays until dismissed. `dialogs` holds the day picker a snooze can open; render it with the table.
export function useActionItemBulkActions(now: number) {
  const { t } = useTranslation([ 'actionItems', 'common' ])
  const dispatch = useAppDispatch()
  useInitiativesLoader()
  const initiatives = useAppSelector(selectLiveInitiatives)
  const isMember = useAppSelector(selectIsCompanyMember)
  const dayPicker = useOverlayState()
  // The selection a day is being picked for, held while the picker is open.
  const pickingFor = useRef<ActionItem[]>([])

  // Resolves to the items changed, or null when a request failed; either way the person
  // has been told, naming the first item refused with `refusalKeys`' words.
  async function send(
    rows: ActionItem[],
    change: ActionItemBulkChange,
    refusalKeys: RefusalKeys = DEFAULT_REFUSAL_KEYS,
  ) {
    const titlesById = new Map(rows.map((row) => [ row.id, row.title ]))
    const itemIds = [ ...titlesById.keys() ]
    const changed: ActionItem[] = []
    const refused: ActionItemRefusal[] = []
    try {
      for (let start = 0; start < itemIds.length; start += ACTION_ITEM_BULK_MAX_ITEMS) {
        const part = itemIds.slice(start, start + ACTION_ITEM_BULK_MAX_ITEMS)
        const response = await changeActionItems(part, change)
        // Filed as each part lands, so a part that fails later leaves the earlier ones shown.
        dispatch(actionItemsUpserted(response.items))
        changed.push(...response.items)
        refused.push(...response.refused)
      }
    }
    catch (error) {
      console.debug('useActionItemBulkActions failed to change a selection', { error, change })
      toast.danger(t('common:errors.unexpected'))
      return null
    }

    if (refused.length) {
      // Usually an item changed elsewhere since the list drew it, such as in another tab.
      console.debug('useActionItemBulkActions: items refused the change', { refused, change })
      const [ first ] = refused
      const title = titlesById.get(first.itemId) ?? first.itemId
      const othersCount = refused.length - 1
      const message = othersCount
        ? t(refusalKeys.more, { title, count: othersCount })
        : t(refusalKeys.one, { title })
      toast.warning(message)
    }
    return changed
  }

  async function snooze(rows: ActionItem[], until: string | null) {
    const changed = await send(rows, { kind: 'snooze', until })
    if (!changed?.length) {
      return Boolean(changed)
    }
    const key = until
      ? 'bulk.toasts.snoozed'
      : 'bulk.toasts.woken'
    toast.success(t(key, { count: changed.length }))
    return true
  }

  async function restore(rows: ActionItem[]) {
    const changed = await send(rows, { kind: 'restore' })
    if (changed?.length) {
      toast.success(t('bulk.toasts.restored', { count: changed.length }))
    }
  }

  const stateAction: DataTableSelectionAction<ActionItem> = {
    id: 'state',
    label: t('bulk.state'),
    icon: <LuCircleDot className='size-4' aria-hidden />,
    choices: BULK_STATES.map((state) => ({ value: state, label: t(stateLabelKeys[state]) })),
    onChoose: async (value, rows) => {
      const state = BULK_STATES.find((candidate) => candidate === value)
      if (!state) {
        console.debug('The bulk state action received a state it does not offer', { value })
        return
      }
      const changed = await send(rows, { kind: 'state', state })
      if (changed?.length) {
        toast.success(t(stateToastKeys[state], { count: changed.length }))
      }
    },
  }

  const priorityAction: DataTableSelectionAction<ActionItem> = {
    id: 'priority',
    label: t('bulk.priority'),
    icon: <LuFlag className='size-4' aria-hidden />,
    choices: ACTION_ITEM_PRIORITIES.map((priority) => ({ value: priority, label: t(priorityLabelKeys[priority]) })),
    onChoose: async (value, rows) => {
      const priority = ACTION_ITEM_PRIORITIES.find((candidate) => candidate === value)
      if (!priority) {
        console.debug('The bulk priority action received a priority it does not offer', { value })
        return
      }
      const changed = await send(rows, { kind: 'priority', priority })
      if (changed?.length) {
        toast.success(t('bulk.toasts.prioritized', {
          count: changed.length,
          priority: t(priorityLabelKeys[priority]),
        }))
      }
    },
  }

  const snoozeAction: DataTableSelectionAction<ActionItem> = {
    id: 'snooze',
    label: t('snooze.label'),
    icon: <LuCalendarClock className='size-4' aria-hidden />,
    choices: [
      ...SNOOZE_PRESETS.map((preset) => ({ value: preset, label: t(snoozePresetLabelKeys[preset]) })),
      { value: PICK_DAY, label: t('snooze.pickDate') },
      { value: END_SNOOZE, label: t('snooze.end') },
    ],
    onChoose: async (value, rows) => {
      if (value === PICK_DAY) {
        pickingFor.current = rows
        dayPicker.open()
        return
      }
      if (value === END_SNOOZE) {
        await snooze(rows, null)
        return
      }
      const preset = SNOOZE_PRESETS.find((candidate) => candidate === value)
      if (!preset) {
        console.debug('The bulk snooze action received a choice it does not offer', { value })
        return
      }
      await snooze(rows, getSnoozePresetInstants(now, getLocalTimeZone())[preset])
    },
  }

  const initiativeChoices = initiatives.map((initiative) => ({ value: initiative.id, label: initiative.name }))
  const initiativeNames = new Map(initiativeChoices.map((choice) => [ choice.value, choice.label ]))

  const joinAction: DataTableSelectionAction<ActionItem> = {
    id: 'joinInitiative',
    label: t('bulk.joinInitiative'),
    icon: <LuFolderInput className='size-4' aria-hidden />,
    isDisabled: !initiativeChoices.length,
    choices: initiativeChoices,
    onChoose: async (initiativeId, rows) => {
      const changed = await send(rows, { kind: 'join-initiative', initiativeId })
      if (changed?.length) {
        toast.success(t('bulk.toasts.joined', {
          count: changed.length,
          initiative: initiativeNames.get(initiativeId) ?? '',
        }))
      }
    },
  }

  const leaveAction: DataTableSelectionAction<ActionItem> = {
    id: 'leaveInitiative',
    label: t('bulk.leaveInitiative'),
    icon: <LuFolderMinus className='size-4' aria-hidden />,
    isDisabled: !initiativeChoices.length,
    choices: initiativeChoices,
    onChoose: async (initiativeId, rows) => {
      const changed = await send(rows, { kind: 'leave-initiative', initiativeId })
      if (changed?.length) {
        toast.success(t('bulk.toasts.left', {
          count: changed.length,
          initiative: initiativeNames.get(initiativeId) ?? '',
        }))
      }
    },
  }

  // Wakes each item's lead project's Project Manager to plan it. Only the company's members see
  // Project Managers, so only they are offered it.
  const assignAction: DataTableSelectionAction<ActionItem> = {
    id: 'assignToManager',
    label: t('bulk.assignToManager'),
    icon: <LuWorkflow className='size-4' aria-hidden />,
    onPress: async (rows) => {
      const assigned = await send(rows, { kind: 'assign-to-manager' }, UNASSIGNABLE_KEYS)
      if (assigned?.length) {
        toast.success(t('bulk.toasts.assigned', { count: assigned.length }))
      }
    },
  }

  // Deleting is soft, so it happens at once, with Undo restoring exactly what it deleted.
  const deleteAction: DataTableSelectionAction<ActionItem> = {
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

  const restoreAction: DataTableSelectionAction<ActionItem> = {
    id: 'restore',
    label: t('list.restore'),
    icon: <LuUndo2 className='size-4' aria-hidden />,
    onPress: restore,
  }

  const dialogs = dayPicker.isOpen && <SnoozeDateModal
    state={dayPicker}
    onSnooze={(until) => snooze(pickingFor.current, until)}
  />

  // Destructive last, as in every menu.
  const liveActions: DataTableSelectionAction<ActionItem>[] = [
    stateAction,
    priorityAction,
    snoozeAction,
    joinAction,
    leaveAction,
    ...(isMember
      ? [ assignAction ]
      : []),
    deleteAction,
  ]
  const deletedActions: DataTableSelectionAction<ActionItem>[] = [ restoreAction ]

  return {
    liveActions,
    deletedActions,
    dialogs,
  } as const
}
