// Copyright © 2026 Jalapeno Labs

import type { InitiativeFormInput, InitiativeFormValues } from './initiativeFormSchema'

// Core
import { useMemo } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../../store/hooks'
import { initiativeUpserted } from '../../store/initiativesSlice'
import { selectAllProjects } from '../../store/projectsSlice'

// User interface
import { Button, FieldError, Form, Input, Label, TextArea, TextField, toast } from '@heroui/react'
import { DayPicker } from '../../components/DayPicker'
import { MultiPicker } from '../../components/MultiPicker'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { getLocalTimeZone } from '@internationalized/date'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { createInitiative } from '../../api/routes/initiativeRoutes'
import { useProjectsLoader } from '../../hooks/useServerData'
import { toEndOfDayInstant } from '../ActionItems/actionItemDates'
import { createInitiativeFormSchema } from './initiativeFormSchema'

type Props = {
  // Projects the initiative starts in, such as the project page it was created from.
  initialProjectIds: string[]
  onSaved: (initiativeId: string) => void
  onCancel: () => void
}

// Starts an initiative. It starts active and empty; items join it from their own pages or
// from the initiative's.
export function InitiativeForm(props: Props) {
  const { t } = useTranslation([ 'initiatives', 'actionItems', 'common' ])
  const dispatch = useAppDispatch()
  useProjectsLoader()
  const projects = useAppSelector(selectAllProjects)

  const resolver = useMemo(() => zodResolver(createInitiativeFormSchema(t)), [ t ])
  const form = useForm<InitiativeFormInput, unknown, InitiativeFormValues>({
    resolver,
    defaultValues: {
      name: '',
      description: '',
      targetDate: null,
      projectIds: props.initialProjectIds,
    },
  })

  const onSubmit = form.handleSubmit(async (values) => {
    try {
      const response = await createInitiative({
        name: values.name,
        description: values.description,
        targetAt: values.targetDate
          ? toEndOfDayInstant(values.targetDate, getLocalTimeZone())
          : null,
        projectIds: values.projectIds,
      })
      dispatch(initiativeUpserted(response.initiative))
      toast.success(t('toasts.created', { name: response.initiative.name }))
      props.onSaved(response.initiative.id)
    }
    catch (error) {
      const message = getApiErrorMessage(error)
      console.debug('InitiativeForm failed to create the initiative', { error })
      toast.danger(t('common:errors.unexpected'), { description: message ?? undefined })
    }
  })

  const errors = form.formState.errors
  // useWatch subscribes per field and, unlike form.watch, is safe for the React Compiler.
  const values = useWatch({ control: form.control })
  // Watched on its own: the whole-form watch types a class instance as a partial copy.
  const targetDate = useWatch({ control: form.control, name: 'targetDate' })

  return <Form onSubmit={onSubmit} validationBehavior='aria' className='grid max-w-3xl grid-cols-1 gap-4'>
    <TextField
      autoFocus
      isRequired
      isInvalid={Boolean(errors.name)}
      value={values.name}
      onChange={(value) => form.setValue('name', value, { shouldDirty: true, shouldValidate: true })}
    >
      <Label>{t('form.name')}</Label>
      <Input />
      <FieldError>{errors.name?.message}</FieldError>
    </TextField>

    <TextField
      isInvalid={Boolean(errors.description)}
      value={values.description}
      onChange={(value) => form.setValue('description', value, { shouldDirty: true, shouldValidate: true })}
    >
      <Label>{t('form.description')}</Label>
      <TextArea rows={4} />
      <FieldError>{errors.description?.message}</FieldError>
    </TextField>

    <div className='grid grid-cols-1 gap-4 sm:grid-cols-2'>
      <DayPicker
        label={t('fields.targetDate')}
        calendarLabel={t('fields.targetDate')}
        description={t('form.targetDateHint')}
        value={targetDate}
        onChange={(date) => form.setValue('targetDate', date, { shouldDirty: true })}
      />
    </div>

    <MultiPicker
      label={t('fields.projects')}
      searchLabel={t('actionItems:fields.searchProjects')}
      emptyLabel={t('actionItems:fields.noProjectsFound')}
      options={projects.map((project) => ({ id: project.id, label: project.name }))}
      value={values.projectIds ?? []}
      onChange={(projectIds) => form.setValue('projectIds', projectIds, { shouldDirty: true })}
    />

    <div className='mt-2 flex justify-end gap-2'>
      <Button variant='tertiary' onPress={props.onCancel}>
        <span>{t('common:actions.cancel')}</span>
      </Button>
      <Button type='submit' isPending={form.formState.isSubmitting}>
        <span>{t('form.submit')}</span>
      </Button>
    </div>
  </Form>
}
