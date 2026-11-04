// Copyright © 2026 Jalapeno Labs

import type { UseFormReturn } from 'react-hook-form'
import type { Initiative, UpdateInitiativeRequest } from '../../api/routes/initiativeRoutes'
import type { InitiativeEditInput, InitiativeEditValues } from './initiativeFormSchema'

// Core
import { useEffect, useMemo } from 'react'
import { useForm } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { initiativeUpserted } from '../../store/initiativesSlice'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { getLocalTimeZone } from '@internationalized/date'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { addInitiativeProject, removeInitiativeProject, updateInitiative } from '../../api/routes/initiativeRoutes'
import { saveForm } from '../../layout/page/pageSave'
import { toChangedDayInstant, toLocalDay } from '../ActionItems/actionItemDates'
import { getMembershipChanges } from '../ActionItems/membershipChanges'
import { createInitiativeEditSchema } from './initiativeFormSchema'
import { toast } from '../../toast/toast'

export type InitiativeEditForm = UseFormReturn<InitiativeEditInput, unknown, InitiativeEditValues>

function toFormValues(initiative: Initiative, timeZone: string): InitiativeEditInput {
  return {
    name: initiative.name,
    description: initiative.description,
    state: initiative.state,
    targetDate: initiative.targetAt
      ? toLocalDay(initiative.targetAt, timeZone)
      : null,
    projectIds: initiative.projectIds,
  }
}

// The initiative page's form: its name, description, state, target date, and projects, kept
// by the page's Save. Fields the person has not touched follow the initiative as the event
// stream changes it. A save sends only what differs from the initiative as it stands, so a
// save that failed part way is finished by the next one rather than repeated.
export function useInitiativeEditor(initiative: Initiative) {
  const { t } = useTranslation([ 'initiatives', 'common' ])
  const dispatch = useAppDispatch()
  const timeZone = getLocalTimeZone()
  const resolver = useMemo(() => zodResolver(createInitiativeEditSchema(t)), [ t ])
  const form: InitiativeEditForm = useForm<InitiativeEditInput, unknown, InitiativeEditValues>({
    resolver,
    defaultValues: toFormValues(initiative, timeZone),
  })

  useEffect(() => {
    form.reset(toFormValues(initiative, timeZone), { keepDirtyValues: true, keepDirty: true })
  }, [ form, initiative, timeZone ])

  const save = saveForm(form, async (values) => {
    const fields: UpdateInitiativeRequest = {}
    if (values.name !== initiative.name) {
      fields.name = values.name
    }
    if (values.description !== initiative.description) {
      fields.description = values.description
    }
    if (values.state !== initiative.state) {
      fields.state = values.state
    }
    const targetAt = toChangedDayInstant(initiative.targetAt, values.targetDate, timeZone)
    if (targetAt !== undefined) {
      fields.targetAt = targetAt
    }
    // Projects are joined and left one at a time; each answer is the initiative as it then stands.
    const projects = getMembershipChanges(initiative.projectIds, values.projectIds)

    try {
      let saved = initiative
      if (Object.keys(fields).length) {
        saved = (await updateInitiative(initiative.id, fields)).initiative
        dispatch(initiativeUpserted(saved))
      }
      for (const projectId of projects.added) {
        saved = (await addInitiativeProject(initiative.id, projectId)).initiative
        dispatch(initiativeUpserted(saved))
      }
      for (const projectId of projects.removed) {
        saved = (await removeInitiativeProject(initiative.id, projectId)).initiative
        dispatch(initiativeUpserted(saved))
      }
      form.reset(toFormValues(saved, timeZone))
      toast.success(t('toasts.saved', { name: saved.name }))
      return true
    }
    catch (error) {
      console.debug('useInitiativeEditor failed to save the initiative', { error, initiativeId: initiative.id })
      toast.danger(t('common:errors.unexpected'), { description: getApiErrorMessage(error) })
      return false
    }
  })

  return { form, save } as const
}
