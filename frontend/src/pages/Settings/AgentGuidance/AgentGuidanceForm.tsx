// Copyright © 2026 Jalapeno Labs

import type { AgentGuidance } from '../../../api/routes/agentGuidanceRoutes'

// Core
import { useEffect, useMemo, useState } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import { useSWRConfig } from 'swr'

// User interface
import { Description, FieldError, Form, Label, TextArea, TextField } from '@heroui/react'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { z } from 'zod'

// Misc
import { getApiErrorCode, getApiErrorMessage } from '../../../api/errors'
import { AGENT_GUIDANCE_KEY, saveAgentGuidance } from '../../../api/routes/agentGuidanceRoutes'
import { numberFormat } from '../../../format/intl'
import { useInitialFocus } from '../../../hooks/useInitialFocus'
import { saveForm, usePageSave } from '../../../layout/page/pageSave'
import { toast } from '../../../toast/toast'

type Props = {
  guidance: AgentGuidance
  // Told whenever the text starts or stops differing from the version it was edited from, so
  // the versions wait for the edit before one is restored.
  onDirtyChange: (isDirty: boolean) => void
}

type Values = {
  content: string
}

// The guidance as admins edit it, saved by the page's Save as the next version. The text follows
// the current version as it changes, until the person edits it; then it stays theirs, made
// against the version they started from, so a save after someone else's tells them so.
export function AgentGuidanceForm(props: Props) {
  const { t } = useTranslation([ 'agentGuidance', 'common' ])
  const { mutate } = useSWRConfig()
  const { onDirtyChange } = props
  const current = props.guidance.current
  const max = props.guidance.maxCharacters
  const formattedMax = numberFormat({}).format(max)

  // Counted by code point, as the API counts characters, and trimmed, as it is stored.
  const resolver = useMemo(() => zodResolver(z.object({
    content: z
      .string()
      .refine((value) => Array.from(value.trim()).length <= max, t('form.tooLong', { max: formattedMax })),
  })), [ max, formattedMax, t ])
  const form = useForm<Values>({ resolver, defaultValues: { content: current.content }})
  const [ baseVersion, setBaseVersion ] = useState(current.version)
  const content = useWatch({ control: form.control, name: 'content' })
  const firstFieldRef = useInitialFocus()
  const isDirty = form.formState.isDirty

  useEffect(() => {
    if (isDirty) {
      return
    }
    form.reset({ content: current.content })
    setBaseVersion(current.version)
  }, [ current.version, current.content, isDirty, form ])

  useEffect(() => {
    onDirtyChange(isDirty)
  }, [ isDirty, onDirtyChange ])

  const save = saveForm(form, async (values) => {
    try {
      const response = await saveAgentGuidance(values.content, baseVersion)
      void mutate(AGENT_GUIDANCE_KEY, response, { revalidate: false })
      // The new defaults are what was saved, trimmed as the API stores it, so Save rests.
      form.reset({ content: response.current.content })
      setBaseVersion(response.current.version)
      toast.success(t('toasts.saved'))
      return true
    }
    catch (error) {
      if (getApiErrorCode(error) === 'guidance_changed') {
        // Every version is kept, so nothing is lost by saving over it once the person knows.
        const latest = await mutate<AgentGuidance>(AGENT_GUIDANCE_KEY)
        if (latest) {
          setBaseVersion(latest.current.version)
          toast.warning(t('toasts.changed', { version: latest.current.version }))
          return false
        }
      }
      console.debug('AgentGuidanceForm could not save the guidance', { error, baseVersion })
      toast.danger(t('common:errors.unexpected'), { description: getApiErrorMessage(error) ?? undefined })
      return false
    }
  })
  const onSubmit = usePageSave({ save, isDirty })
  const error = form.formState.errors.content

  return <Form onSubmit={onSubmit} validationBehavior='aria'>
    <TextField
      id='agent-guidance-content'
      className='w-full'
      isInvalid={Boolean(error)}
      value={content ?? ''}
      onChange={(value) => form.setValue('content', value, { shouldDirty: true, shouldValidate: true })}
    >
      <Label>{t('form.label')}</Label>
      <TextArea ref={firstFieldRef} rows={14} autoComplete='off' />
      <Description>{
        t('form.length', {
          length: numberFormat({}).format(Array.from((content ?? '').trim()).length),
          max: formattedMax,
        })
      }</Description>
      <FieldError>{error?.message}</FieldError>
    </TextField>
  </Form>
}
