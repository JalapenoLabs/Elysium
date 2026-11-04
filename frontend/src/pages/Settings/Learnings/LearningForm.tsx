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
import { Description, FieldError, Form, Input, Label, TextArea, TextField } from '@heroui/react'
import { OptionSelect } from '../../../components/OptionSelect'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { HTTPError } from 'ky'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { BUILT_IN_LEARNING_CATEGORIES, correctLearning, saveLearning } from '../../../api/routes/learningRoutes'
import { useLearningCategories } from '../../../hooks/useLearningCategories'
import { usePageSave } from '../../../layout/page/pageSave'
import { createLearningFormSchema } from './learningFormSchema'
import { LEARNING_FIELDS } from './learningPresentation'
import { forgetNewLearningDraft, readNewLearningDraft, saveNewLearningDraft } from './newLearningDraft'
import { toast } from '../../../toast/toast'

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
        // The first built-in category, which every workspace has.
        category: BUILT_IN_LEARNING_CATEGORIES[0],
        body: '',
        evidence: '',
      },
  })
  const { status: categoriesStatus, categories } = useLearningCategories()

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
  const category = values.category ?? BUILT_IN_LEARNING_CATEGORIES[0]
  const chosen = categories.find((option) => option.key === category)

  // A category that no longer exists, such as one a draft kept after it was removed, gives way
  // to the first there is, so the form never holds a key the API would refuse.
  useEffect(() => {
    if (categoriesStatus !== 'loaded' || chosen || !categories.length) {
      return
    }
    console.debug('LearningForm replaced a category that no longer exists', { category })
    form.setValue('category', categories[0].key, { shouldDirty: true, shouldValidate: true })
  }, [ categoriesStatus, chosen, categories, category, form ])

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

  usePageSave({
    formId: LEARNING_FORM_ID,
    isDirty: form.formState.isDirty,
    isPending: form.formState.isSubmitting,
  })

  const errors = form.formState.errors
  const categoryOptions = categories.map((option) => ({
    id: option.key,
    label: option.label,
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
        isDisabled={categoriesStatus !== 'loaded'}
        onChange={(value) => form.setValue('category', value, { shouldDirty: true, shouldValidate: true })}
      />
      {chosen && <p className='mt-1 text-xs opacity-70'>{
        chosen.description
      }</p>}
      {categoriesStatus === 'failed' && <p className='mt-1 text-xs text-danger'>{
        t('form.errors.categoriesUnavailable')
      }</p>}
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
