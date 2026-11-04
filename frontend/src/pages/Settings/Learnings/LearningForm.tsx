// Copyright © 2026 Jalapeno Labs

import type { Learning, SaveOutcome } from '../../../api/routes/learningRoutes'
import type { LearningFormInput, LearningFormValues } from './learningFormSchema'

// Core
import { useEffect, useMemo } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { learningUpserted } from '../../../store/learningsSlice'

// User interface
import { Description, FieldError, Form, Input, Label, TextArea, TextField, toast } from '@heroui/react'
import { OptionSelect } from '../../../components/OptionSelect'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { HTTPError } from 'ky'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { correctLearning, LEARNING_CATEGORIES, saveLearning } from '../../../api/routes/learningRoutes'
import { useSettingsSave } from '../settingsSave'
import { createLearningFormSchema } from './learningFormSchema'
import { categoryDescriptionKeys, categoryLabelKeys, LEARNING_FIELDS } from './learningPresentation'
import { forgetNewLearningDraft, readNewLearningDraft, saveNewLearningDraft } from './newLearningDraft'

const LEARNING_FORM_ID = 'learning-form'

const saveToastKeys = {
  created: 'toasts.created',
  revised: 'toasts.revised',
  unchanged: 'toasts.unchanged',
} as const satisfies Record<SaveOutcome, string>

type Props = {
  // The learning being corrected, or null to save a new one.
  learning: Learning | null
  // Called once a save lands, such as to open the learning saved.
  onSaved?: (learning: Learning) => void
}

// Saves a new learning, or corrects one. A new learning's takeaway that a learning already has
// revises that one, as saving it from an agent does. Saved from the page's Save, in the header.
// While new, what is typed is kept in session storage until it is saved.
export function LearningForm(props: Props) {
  const { t } = useTranslation([ 'learnings', 'common' ])
  const dispatch = useAppDispatch()
  const stored = props.learning

  const resolver = useMemo(() => zodResolver(createLearningFormSchema(t)), [ t ])
  const form = useForm<LearningFormInput, unknown, LearningFormValues>({
    resolver,
    defaultValues: stored
      ? {
        title: stored.title,
        category: stored.category,
        body: stored.body,
        evidence: stored.evidence,
      }
      : {
        title: '',
        category: LEARNING_CATEGORIES[0],
        body: '',
        evidence: '',
      },
  })

  // A new learning's draft comes back as changes to the blank form, so Save is ready for it.
  useEffect(() => {
    if (stored) {
      return
    }
    const draft = readNewLearningDraft()
    if (!draft) {
      return
    }
    for (const field of LEARNING_FIELDS) {
      form.setValue(field, draft[field], { shouldDirty: true })
    }
  }, [ stored, form ])

  // useWatch subscribes per field and, unlike form.watch, is safe for the React Compiler.
  const values = useWatch({ control: form.control })
  const category = values.category ?? LEARNING_CATEGORIES[0]

  useEffect(() => {
    if (stored) {
      return
    }
    saveNewLearningDraft(values)
  }, [ stored, values ])

  const onSubmit = form.handleSubmit(async (submitted) => {
    try {
      if (stored) {
        const response = await correctLearning(stored.id, submitted)
        dispatch(learningUpserted(response.learning))
        // The form's new defaults are what was saved, so Save rests until the next change.
        form.reset(submitted)
        toast.success(t('toasts.corrected'))
        props.onSaved?.(response.learning)
        return
      }

      const response = await saveLearning(submitted)
      dispatch(learningUpserted(response.learning))
      forgetNewLearningDraft()
      toast.success(t(saveToastKeys[response.outcome]))
      props.onSaved?.(response.learning)
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 409) {
        form.setError('title', { message: t('form.errors.titleTaken') })
        return
      }
      // The form mirrors the API's rules, so a refusal here is one it did not foresee; its
      // message still names what to fix.
      const message = getApiErrorMessage(error)
      console.debug('LearningForm failed to save the learning', { error })
      toast.danger(t('common:errors.unexpected'), { description: message ?? undefined })
    }
  })

  useSettingsSave({
    formId: LEARNING_FORM_ID,
    isDirty: form.formState.isDirty,
    isPending: form.formState.isSubmitting,
  })

  const errors = form.formState.errors
  const categoryOptions = LEARNING_CATEGORIES.map((option) => ({
    id: option,
    label: t(categoryLabelKeys[option]),
  }))

  return <Form
    id={LEARNING_FORM_ID}
    onSubmit={onSubmit}
    validationBehavior='aria'
    className='flex max-w-3xl flex-col gap-4'
  >
    {/* Takeaway */}
    <TextField
      id='learning-title'
      autoFocus
      isRequired
      isInvalid={Boolean(errors.title)}
      value={values.title}
      onChange={(value) => form.setValue('title', value, { shouldDirty: true, shouldValidate: true })}
    >
      <Label>{t('form.title')}</Label>
      <Input autoComplete='off' />
      <Description>{t('form.titleHint')}</Description>
      <FieldError>{errors.title?.message}</FieldError>
    </TextField>

    {/* Category */}
    <div id='learning-category'>
      <OptionSelect
        label={t('form.category')}
        options={categoryOptions}
        value={category}
        onChange={(value) => {
          const picked = LEARNING_CATEGORIES.find((option) => option === value)
          if (!picked) {
            console.debug('LearningForm ignored an unknown category', { value })
            return
          }
          form.setValue('category', picked, { shouldDirty: true, shouldValidate: true })
        }}
      />
      <p className='mt-1 text-xs opacity-70'>{
        t(categoryDescriptionKeys[category])
      }</p>
    </div>

    {/* Body */}
    <TextField
      id='learning-body'
      isRequired
      isInvalid={Boolean(errors.body)}
      value={values.body}
      onChange={(value) => form.setValue('body', value, { shouldDirty: true, shouldValidate: true })}
    >
      <Label>{t('form.body')}</Label>
      <TextArea rows={8} />
      <Description>{t('form.bodyHint')}</Description>
      <FieldError>{errors.body?.message}</FieldError>
    </TextField>

    {/* Evidence */}
    <TextField
      id='learning-evidence'
      isRequired
      isInvalid={Boolean(errors.evidence)}
      value={values.evidence}
      onChange={(value) => form.setValue('evidence', value, { shouldDirty: true, shouldValidate: true })}
    >
      <Label>{t('form.evidence')}</Label>
      <TextArea rows={4} />
      <Description>{t('form.evidenceHint')}</Description>
      <FieldError>{errors.evidence?.message}</FieldError>
    </TextField>
  </Form>
}
