// Copyright © 2026 Jalapeno Labs

import type { WorkBatch } from '../../api/routes/projectManagerRoutes'
import type { WorkBatchEditOutput, WorkBatchEditValues } from './workBatchEditSchema'

// Core
import { useMemo, useState } from 'react'
import { useFieldArray, useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import { mutate } from 'swr'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { workBatchLoaded } from '../../store/workQueueSlice'

// User interface
import { Button, FieldError, Form, Input, Label, TextArea, TextField } from '@heroui/react'
import { LuPlus } from 'react-icons/lu'
import { EmptyNotice } from '../../components/EmptyNotice'
import { OptionSelect } from '../../components/OptionSelect'
import { BatchRepositoryRow } from './BatchRepositoryRow'
import { BatchScheduleField } from './BatchScheduleField'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { getProjectManagerKey } from '../../api/routes/projectManagerRoutes'
import { approveWorkBatch, WORK_QUEUE_KEY } from '../../api/routes/workQueueRoutes'
import { useInitialFocus } from '../../hooks/useInitialFocus'
import { saveForm, usePageSave } from '../../layout/page/pageSave'
import { toast } from '../../toast/toast'
import { getWorkBatchViewUrl } from '../../urls'
import { MAX_SESSION_REPOSITORIES } from '../Coding/sessionRepositories'
import { batchPriorityLabelKeys } from '../Projects/projectManagerPresentation'
import {
  BATCH_GOAL_MAX_CHARACTERS,
  BATCH_TITLE_MAX_CHARACTERS,
  createWorkBatchEditSchema,
  toWorkBatchEdits,
  toWorkBatchEditValues,
} from './workBatchEditSchema'

const PRIORITIES = [ 'asap', 'normal', 'scheduled' ] as const

type Props = {
  batch: WorkBatch
}

// A proposed batch's title, goal, repositories, and priority as one form, which the page's Save,
// named Approve, sends with the approval: what the person changed replaces what its Project
// Manager proposed, and the rest stays. Approving it unchanged is fine too. Once approved, the
// page goes back to the batch.
//
// Nothing is kept as a draft: like every form editing a record that exists, it starts from what is
// saved (`docs/frontend.md`, Forms).
export function WorkBatchEditForm(props: Props) {
  const { t } = useTranslation([ 'workQueue', 'common' ])
  const codingTranslation = useTranslation('coding')
  const projectsTranslation = useTranslation('projects')
  const dispatch = useAppDispatch()
  const navigate = useNavigate()
  // When the page opened: a scheduled time must still be ahead of it.
  const [ openedAt ] = useState(() => Date.now())
  const schema = useMemo(
    () => createWorkBatchEditSchema(t, codingTranslation.t, openedAt, props.batch),
    [ t, codingTranslation.t, openedAt, props.batch ],
  )
  const form = useForm<WorkBatchEditValues, unknown, WorkBatchEditOutput>({
    resolver: zodResolver(schema),
    defaultValues: toWorkBatchEditValues(props.batch),
  })
  const repositories = useFieldArray({ control: form.control, name: 'repositories' })
  const values = useWatch({ control: form.control })
  const titleRef = useInitialFocus()
  const errors = form.formState.errors

  // Checked as it stands, so Approve explains what holds it before it is pressed, and a rule over
  // the whole list shows under it as soon as it is broken.
  const checked = schema.safeParse(values)
  const issues = checked.error?.issues ?? []
  const listIssue = issues.find((issue) => issue.path.length === 1 && issue.path[0] === 'repositories')

  const save = saveForm(form, async (output) => {
    try {
      const response = await approveWorkBatch(props.batch.id, toWorkBatchEdits(props.batch, output))
      dispatch(workBatchLoaded(response))
      void mutate(WORK_QUEUE_KEY)
      void mutate(getProjectManagerKey(props.batch.projectId))
      // What was approved is what the form holds now, so leaving asks nothing. The edit page is
      // done with once its batch is approved, so the batch's page takes its place in the history.
      form.reset(toWorkBatchEditValues(response.batch))
      toast.success(t('actions.done.approve', { title: response.batch.title }))
      void navigate(getWorkBatchViewUrl(props.batch.id), { replace: true })
      return true
    }
    catch (error) {
      const message = getApiErrorMessage(error)
      if (!message) {
        console.debug('WorkBatchEditForm could not approve a batch', { error, batchId: props.batch.id })
      }
      toast.danger(message ?? t('common:errors.unexpected'))
      return false
    }
  })
  const onSubmit = usePageSave({
    save,
    isDirty: form.formState.isDirty,
    blockedReason: issues[0]?.message ?? null,
    label: t('actions.approve'),
    canSaveUnchanged: true,
  })

  const isFull = repositories.fields.length >= MAX_SESSION_REPOSITORIES
  const priority = values.priority ?? props.batch.priority

  return <Form className='max-w-3xl' onSubmit={onSubmit} validationBehavior='aria'>
    <div className='compact w-full'>
      <TextField
        id='work-batch-title'
        isRequired
        isInvalid={Boolean(errors.title)}
        value={values.title ?? ''}
        onChange={(value) => form.setValue('title', value, { shouldDirty: true, shouldValidate: true })}
      >
        <Label>{t('edit.title')}</Label>
        <Input ref={titleRef} maxLength={BATCH_TITLE_MAX_CHARACTERS} autoComplete='off' />
        <FieldError>{errors.title?.message}</FieldError>
      </TextField>
    </div>

    <div className='compact w-full'>
      <TextField
        id='work-batch-goal'
        isRequired
        isInvalid={Boolean(errors.goal)}
        value={values.goal ?? ''}
        onChange={(value) => form.setValue('goal', value, { shouldDirty: true, shouldValidate: true })}
      >
        <Label>{t('edit.goal')}</Label>
        <TextArea rows={12} maxLength={BATCH_GOAL_MAX_CHARACTERS} autoComplete='off' />
        <FieldError>{errors.goal?.message}</FieldError>
      </TextField>
    </div>

    <div className='relaxed flex w-full flex-col gap-3 sm:flex-row sm:items-start'>
      <OptionSelect
        id='work-batch-priority'
        className='sm:w-56'
        label={t('edit.priority')}
        options={PRIORITIES.map((option) => ({
          id: option,
          label: projectsTranslation.t(batchPriorityLabelKeys[option]),
        }))}
        value={priority}
        onChange={(value) => {
          const picked = PRIORITIES.find((option) => option === value) ?? 'normal'
          form.setValue('priority', picked, { shouldDirty: true, shouldValidate: true })
          // A time goes only with a scheduled batch, so another priority lets it go.
          if (picked !== 'scheduled') {
            form.setValue('scheduledFor', null, { shouldDirty: true, shouldValidate: true })
          }
        }}
      />
      {priority === 'scheduled' && <div className='min-w-0 flex-1'>
        <BatchScheduleField
          value={values.scheduledFor ?? null}
          errorMessage={errors.scheduledFor?.message}
          onChange={(value) => form.setValue('scheduledFor', value, { shouldDirty: true, shouldValidate: true })}
        />
      </div>}
    </div>

    <section className='relaxed w-full'>
      <h2 className='compact text-lg font-semibold'>{t('batch.repositories')}</h2>
      {repositories.fields.length
        ? <ol className='compact'>{
          repositories.fields.map((field, index) => {
            const rowErrors = errors.repositories?.[index]
            return <BatchRepositoryRow
              // Rows are added and removed, so each keeps the form's own key for it.
              key={field.id}
              index={index}
              url={values.repositories?.[index]?.url ?? ''}
              baseBranch={values.repositories?.[index]?.baseBranch ?? ''}
              errors={{
                url: rowErrors?.url?.message,
                baseBranch: rowErrors?.baseBranch?.message,
              }}
              onChange={(name, value) => form.setValue(
                `repositories.${index}.${name}`,
                value,
                { shouldDirty: true, shouldValidate: true },
              )}
              onRemove={() => repositories.remove(index)}
            />
          })
        }</ol>
        : <div className='compact'>
          <EmptyNotice>{t('batch.noRepositories')}</EmptyNotice>
        </div>}
      {listIssue && <p className='compact text-sm text-danger'>{listIssue.message}</p>}
      <Button
        variant='outline'
        isDisabled={isFull}
        onPress={() => repositories.append({ url: '', baseBranch: '' })}
      >
        <LuPlus className='size-4' aria-hidden />
        <span>{t('edit.addRepository')}</span>
      </Button>
    </section>
  </Form>
}
