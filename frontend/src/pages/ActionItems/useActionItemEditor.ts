// Copyright © 2026 Jalapeno Labs

import type { UseFormReturn } from 'react-hook-form'
import type { ActionItem, UpdateActionItemRequest } from '../../api/routes/actionItemRoutes'
import type { ActionItemFormInput, ActionItemFormValues } from './actionItemFormSchema'

// Core
import { useEffect, useMemo } from 'react'
import { useForm } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { actionItemUpserted } from '../../store/actionItemsSlice'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { getLocalTimeZone } from '@internationalized/date'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import {
  addActionItemProject,
  joinInitiative,
  leaveInitiative,
  removeActionItemProject,
  updateActionItem,
} from '../../api/routes/actionItemRoutes'
import { saveForm } from '../../layout/page/pageSave'
import { toChangedDayInstant, toLocalDay } from './actionItemDates'
import { createActionItemFormSchema } from './actionItemFormSchema'
import { getMembershipChanges } from './membershipChanges'
import { toast } from '../../toast/toast'

export type ActionItemEditForm = UseFormReturn<ActionItemFormInput, unknown, ActionItemFormValues>

function toFormValues(item: ActionItem, timeZone: string): ActionItemFormInput {
  return {
    title: item.title,
    notes: item.notes,
    priority: item.priority,
    dueDate: item.dueAt
      ? toLocalDay(item.dueAt, timeZone)
      : null,
    projectIds: item.projectIds,
    initiativeIds: item.initiativeIds,
  }
}

// The item page's form: its title, notes, priority, due date, projects, and initiatives,
// kept by the page's Save. Fields the person has not touched follow the item as the event
// stream changes it. A save sends only what differs from the item as it stands, so a save
// that failed part way is finished by the next one rather than repeated.
export function useActionItemEditor(item: ActionItem) {
  const { t } = useTranslation([ 'actionItems', 'common' ])
  const dispatch = useAppDispatch()
  const timeZone = getLocalTimeZone()
  const resolver = useMemo(() => zodResolver(createActionItemFormSchema(t)), [ t ])
  const form: ActionItemEditForm = useForm<ActionItemFormInput, unknown, ActionItemFormValues>({
    resolver,
    defaultValues: toFormValues(item, timeZone),
  })

  useEffect(() => {
    form.reset(toFormValues(item, timeZone), { keepDirtyValues: true, keepDirty: true })
  }, [ form, item, timeZone ])

  const save = saveForm(form, async (values) => {
    const fields: UpdateActionItemRequest = {}
    if (values.title !== item.title) {
      fields.title = values.title
    }
    if (values.notes !== item.notes) {
      fields.notes = values.notes
    }
    if (values.priority !== item.priority) {
      fields.priority = values.priority
    }
    const dueAt = toChangedDayInstant(item.dueAt, values.dueDate, timeZone)
    if (dueAt !== undefined) {
      fields.dueAt = dueAt
    }
    // Memberships are joined and left one at a time; each answer is the item as it then stands.
    const projects = getMembershipChanges(item.projectIds, values.projectIds)
    const initiatives = getMembershipChanges(item.initiativeIds, values.initiativeIds)

    try {
      let saved = item
      if (Object.keys(fields).length) {
        saved = (await updateActionItem(item.id, fields)).item
        dispatch(actionItemUpserted(saved))
      }
      for (const projectId of projects.added) {
        saved = (await addActionItemProject(item.id, projectId)).item
        dispatch(actionItemUpserted(saved))
      }
      for (const projectId of projects.removed) {
        saved = (await removeActionItemProject(item.id, projectId)).item
        dispatch(actionItemUpserted(saved))
      }
      for (const initiativeId of initiatives.added) {
        saved = (await joinInitiative(item.id, initiativeId)).item
        dispatch(actionItemUpserted(saved))
      }
      for (const initiativeId of initiatives.removed) {
        saved = (await leaveInitiative(item.id, initiativeId)).item
        dispatch(actionItemUpserted(saved))
      }
      form.reset(toFormValues(saved, timeZone))
      toast.success(t('toasts.saved', { title: saved.title }))
      return true
    }
    catch (error) {
      console.debug('useActionItemEditor failed to save the item', { error, itemId: item.id })
      toast.danger(t('common:errors.unexpected'), { description: getApiErrorMessage(error) })
      return false
    }
  })

  return { form, save } as const
}
