// Copyright © 2026 Jalapeno Labs

import type { ActionItemFormInput, ActionItemFormValues } from './actionItemFormSchema'

// Core
import { useMemo } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../../store/hooks'
import { actionItemUpserted } from '../../store/actionItemsSlice'
import { selectLiveInitiatives } from '../../store/initiativesSlice'
import { selectAllProjects } from '../../store/projectsSlice'

// User interface
import { FieldError, Form, Input, Label, TextArea, TextField } from '@heroui/react'
import { PageCreateFooter } from '../../layout/page/PageCreateFooter'
import { DayPicker } from '../../components/DayPicker'
import { MultiPicker } from '../../components/MultiPicker'
import { OptionSelect } from '../../components/OptionSelect'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { getLocalTimeZone } from '@internationalized/date'

// Misc
import { useFormDraft } from '../../drafts/useFormDraft'
import { getApiErrorMessage } from '../../api/errors'
import { ACTION_ITEM_PRIORITIES, createActionItem } from '../../api/routes/actionItemRoutes'
import { useInitialFocus } from '../../hooks/useInitialFocus'
import { useInitiativesLoader, useProjectsLoader } from '../../hooks/useServerData'
import { toEndOfDayInstant } from './actionItemDates'
import { createActionItemFormSchema } from './actionItemFormSchema'
import { priorityLabelKeys } from './actionItemPresentation'
import { saveForm, usePageSave } from '../../layout/page/pageSave'
import { toast } from '../../toast/toast'

type Props = {
  // Projects and initiatives the item starts in, such as the page it was created from.
  initialProjectIds: string[]
  initialInitiativeIds: string[]
  onSaved: (itemId: string) => void
}

// Adds an item by hand, from the header's Save or the Create below the form: two places,
// one action. It starts open, since creating it accepts it. Everything past the title can
// wait; the item's page edits it all later.
export function ActionItemForm(props: Props) {
  const { t } = useTranslation([ 'actionItems', 'common' ])
  const dispatch = useAppDispatch()
  useProjectsLoader()
  useInitiativesLoader()
  const projects = useAppSelector(selectAllProjects)
  const initiatives = useAppSelector(selectLiveInitiatives)
  const titleRef = useInitialFocus()

  const resolver = useMemo(() => zodResolver(createActionItemFormSchema(t)), [ t ])
  const form = useForm<ActionItemFormInput, unknown, ActionItemFormValues>({
    resolver,
    defaultValues: {
      title: '',
      notes: '',
      priority: 'normal',
      dueDate: null,
      projectIds: props.initialProjectIds,
      initiativeIds: props.initialInitiativeIds,
    },
  })
  // What is typed is kept for the tab until the item is created.
  const forgetDraft = useFormDraft(form, { name: 'action-item', fields: [ 'title', 'notes' ], choices: [ 'priority' ]})

  const save = saveForm(form, async (values) => {
    try {
      const response = await createActionItem({
        title: values.title,
        notes: values.notes,
        priority: values.priority,
        dueAt: values.dueDate
          ? toEndOfDayInstant(values.dueDate, getLocalTimeZone())
          : null,
        projectIds: values.projectIds,
        initiativeIds: values.initiativeIds,
      })
      dispatch(actionItemUpserted(response.item))
      forgetDraft()
      toast.success(t('toasts.created', { title: response.item.title }))
      props.onSaved(response.item.id)
      return true
    }
    catch (error) {
      // The form mirrors the API's rules, so a 400 here is one it did not foresee, such as a
      // project deleted since the page loaded; its message still names what to fix.
      const message = getApiErrorMessage(error)
      console.debug('ActionItemForm failed to create the item', { error })
      toast.danger(t('common:errors.unexpected'), { description: message ?? undefined })
      return false
    }
  })

  const onSubmit = usePageSave({
    save,
    onDiscard: forgetDraft,
    isDirty: form.formState.isDirty,
    label: t('form.submit'),
  })

  const errors = form.formState.errors
  // useWatch subscribes per field and, unlike form.watch, is safe for the React Compiler.
  const values = useWatch({ control: form.control })
  // Watched on its own: the whole-form watch types a class instance as a partial copy.
  const dueDate = useWatch({ control: form.control, name: 'dueDate' })

  return <Form
    onSubmit={onSubmit}
    validationBehavior='aria'
    className='grid max-w-3xl grid-cols-1 gap-4'
  >
    <TextField
      id='action-item-title'
      autoComplete='off'
      isRequired
      isInvalid={Boolean(errors.title)}
      value={values.title}
      onChange={(value) => form.setValue('title', value, { shouldDirty: true, shouldValidate: true })}
    >
      <Label>{t('form.title')}</Label>
      <Input ref={titleRef} />
      <FieldError>{errors.title?.message}</FieldError>
    </TextField>

    <TextField
      id='action-item-notes'
      isInvalid={Boolean(errors.notes)}
      value={values.notes}
      onChange={(value) => form.setValue('notes', value, { shouldDirty: true, shouldValidate: true })}
    >
      <Label>{t('form.notes')}</Label>
      <TextArea rows={4} />
      <FieldError>{errors.notes?.message}</FieldError>
    </TextField>

    <div className='grid grid-cols-1 gap-4 sm:grid-cols-2'>
      <OptionSelect
        id='action-item-priority'
        label={t('fields.priority')}
        options={ACTION_ITEM_PRIORITIES.map((priority) => ({ id: priority, label: t(priorityLabelKeys[priority]) }))}
        value={values.priority ?? 'normal'}
        onChange={(value) => {
          const priority = ACTION_ITEM_PRIORITIES.find((option) => option === value)
          if (priority) {
            form.setValue('priority', priority, { shouldDirty: true })
          }
        }}
      />
      <DayPicker
        id='action-item-due-date'
        label={t('fields.dueDate')}
        calendarLabel={t('fields.dueDate')}
        description={t('form.dueDateHint')}
        value={dueDate}
        onChange={(date) => form.setValue('dueDate', date, { shouldDirty: true })}
      />
    </div>

    <MultiPicker
      id='action-item-projects'
      label={t('fields.projects')}
      searchLabel={t('fields.searchProjects')}
      emptyLabel={t('fields.noProjectsFound')}
      options={projects.map((project) => ({ id: project.id, label: project.name }))}
      value={values.projectIds ?? []}
      onChange={(projectIds) => form.setValue('projectIds', projectIds, { shouldDirty: true })}
    />

    <MultiPicker
      id='action-item-initiatives'
      label={t('fields.initiatives')}
      searchLabel={t('fields.searchInitiatives')}
      emptyLabel={t('fields.noInitiativesFound')}
      options={initiatives.map((initiative) => ({ id: initiative.id, label: initiative.name }))}
      value={values.initiativeIds ?? []}
      onChange={(initiativeIds) => form.setValue('initiativeIds', initiativeIds, { shouldDirty: true })}
    />

    <PageCreateFooter />
  </Form>
}
