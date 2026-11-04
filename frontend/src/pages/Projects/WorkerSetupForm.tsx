// Copyright © 2026 Jalapeno Labs

import type { WorkerSetup } from '../../api/routes/workerSetupRoutes'
import type { WorkerSetupFormOutput, WorkerSetupFormValues } from './workerSetupFormSchema'

// Core
import { useEffect, useMemo, useRef } from 'react'
import { useFieldArray, useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import { useSWRConfig } from 'swr'

// User interface
import { Button, Description, FieldError, Form, Label, TextArea, TextField } from '@heroui/react'
import { LuPlus } from 'react-icons/lu'
import { EmptyNotice } from '../../components/EmptyNotice'
import { WorkerRepositoryRow } from './WorkerRepositoryRow'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { getProjectWorkerSetupKey, replaceProjectWorkerSetup } from '../../api/routes/workerSetupRoutes'
import { useInitialFocus } from '../../hooks/useInitialFocus'
import { saveForm, usePageSave } from '../../layout/page/pageSave'
import { toast } from '../../toast/toast'
import {
  createWorkerSetupSchema,
  MAX_WORKER_REPOSITORIES,
  toWorkerSetupFormValues,
  toWorkerSetupRequest,
} from './workerSetupFormSchema'

type Props = {
  projectId: string
  setup: WorkerSetup
}

// The project's worker setup as one form, saved whole by the page's Save, as the API replaces it
// whole. It follows the setup as it changes until the person edits it; then it stays theirs.
//
// Nothing is kept as a draft: like every form editing a record that exists, it starts from what is
// saved (`useFormDraft`, `docs/frontend.md`, Forms).
export function WorkerSetupForm(props: Props) {
  const { t } = useTranslation([ 'projects', 'common' ])
  const codingTranslation = useTranslation('coding')
  const { mutate } = useSWRConfig()
  const schema = useMemo(
    () => createWorkerSetupSchema(t, codingTranslation.t),
    [ t, codingTranslation.t ],
  )
  const form = useForm<WorkerSetupFormValues, unknown, WorkerSetupFormOutput>({
    resolver: zodResolver(schema),
    defaultValues: toWorkerSetupFormValues(props.setup),
  })
  const repositories = useFieldArray({ control: form.control, name: 'repositories' })
  const values = useWatch({ control: form.control })
  const toolsScriptRef = useInitialFocus()
  const isDirty = form.formState.isDirty
  const errors = form.formState.errors

  // The values the form last took from the setup, as text. A reset gives every row a new key,
  // remounting it, so the form resets only for a setup that says something new: never merely for
  // turning clean, as typing a letter and deleting it does, nor for the same setup read again, as
  // the event after the person's own save brings.
  const appliedValuesRef = useRef(JSON.stringify(toWorkerSetupFormValues(props.setup)))
  useEffect(() => {
    const incoming = toWorkerSetupFormValues(props.setup)
    const incomingText = JSON.stringify(incoming)
    if (isDirty || appliedValuesRef.current === incomingText) {
      return
    }
    appliedValuesRef.current = incomingText
    form.reset(incoming)
  }, [ props.setup, isDirty, form ])

  // Checked as it stands, so Save explains what holds it before it is pressed, and a rule over
  // the whole list (too many, the same repository twice) shows under the list as soon as it is
  // broken, as no one field can show it.
  const checked = schema.safeParse(values)
  const issues = checked.error?.issues ?? []
  const listIssue = issues.find((issue) => issue.path.length === 1 && issue.path[0] === 'repositories')

  const save = saveForm(form, async (output) => {
    try {
      const response = await replaceProjectWorkerSetup(props.projectId, toWorkerSetupRequest(output))
      // The new defaults are what was saved, as the API stores it, so Save rests.
      const saved = toWorkerSetupFormValues(response.workerSetup)
      appliedValuesRef.current = JSON.stringify(saved)
      form.reset(saved)
      void mutate(getProjectWorkerSetupKey(props.projectId), response, { revalidate: false })
      toast.success(t('workers.saved'))
      return true
    }
    catch (error) {
      const message = getApiErrorMessage(error)
      if (!message) {
        console.debug('WorkerSetupForm could not save the worker setup', { error, projectId: props.projectId })
      }
      toast.danger(message ?? t('common:errors.unexpected'))
      return false
    }
  })
  const onSubmit = usePageSave({
    save,
    isDirty,
    blockedReason: issues[0]?.message ?? null,
  })

  const isFull = repositories.fields.length >= MAX_WORKER_REPOSITORIES

  return <Form className='max-w-4xl' onSubmit={onSubmit} validationBehavior='aria'>
    <section className='relaxed w-full'>
      <TextField
        id='worker-setup-tools-script'
        isInvalid={Boolean(errors.toolsSetupScript)}
        value={values.toolsSetupScript ?? ''}
        onChange={(value) => form.setValue('toolsSetupScript', value, { shouldDirty: true, shouldValidate: true })}
      >
        <Label>{t('workers.toolsScript')}</Label>
        <TextArea
          ref={toolsScriptRef}
          className='font-mono text-sm'
          rows={8}
          autoComplete='off'
          spellCheck={false}
        />
        <Description>{t('workers.toolsScriptHint')}</Description>
        <FieldError>{errors.toolsSetupScript?.message}</FieldError>
      </TextField>
    </section>

    <section className='relaxed w-full'>
      <h2 className='compact text-lg font-semibold'>{t('workers.repositories')}</h2>
      {repositories.fields.length
        ? <ol className='compact'>{
          repositories.fields.map((field, index) => {
            const rowErrors = errors.repositories?.[index]
            return <WorkerRepositoryRow
              // Rows are edited, added, and moved, so each keeps the form's own key for it.
              key={field.id}
              index={index}
              count={repositories.fields.length}
              value={{
                url: values.repositories?.[index]?.url ?? '',
                baseBranch: values.repositories?.[index]?.baseBranch ?? '',
                setupScript: values.repositories?.[index]?.setupScript ?? '',
              }}
              errors={{
                url: rowErrors?.url?.message,
                baseBranch: rowErrors?.baseBranch?.message,
                setupScript: rowErrors?.setupScript?.message,
              }}
              onChange={(name, value) => form.setValue(
                `repositories.${index}.${name}`,
                value,
                { shouldDirty: true, shouldValidate: true },
              )}
              onMove={(to) => repositories.move(index, to)}
              onRemove={() => repositories.remove(index)}
            />
          })
        }</ol>
        : <div className='compact'>
          <EmptyNotice>{t('workers.noRepositories')}</EmptyNotice>
        </div>}
      {listIssue && <p className='compact text-sm text-danger'>{listIssue.message}</p>}
      <Button
        variant='outline'
        isDisabled={isFull}
        onPress={() => repositories.append({ url: '', baseBranch: '', setupScript: '' })}
      >
        <LuPlus className='size-4' aria-hidden />
        <span>{t('workers.add')}</span>
      </Button>
      {isFull && <p className='mt-2 text-sm opacity-70'>{
        t('workers.errors.tooMany', { max: MAX_WORKER_REPOSITORIES })
      }</p>}
    </section>
  </Form>
}
