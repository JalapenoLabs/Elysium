// Copyright © 2026 Jalapeno Labs

import type { LearningCategory } from '../../../api/routes/learningCategoryRoutes'
import type { LearningCategoryFormInput, LearningCategoryFormValues } from './learningCategoryFormSchema'

// Core
import { useEffect, useMemo, useRef, useState } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { learningCategoryUpserted } from '../../../store/learningCategoriesSlice'

// User interface
import { Description, FieldError, Form, Input, Label, TextArea, TextField } from '@heroui/react'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { HTTPError } from 'ky'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { createLearningCategory, updateLearningCategory } from '../../../api/routes/learningCategoryRoutes'
import { saveForm, usePageSave } from '../../../layout/page/pageSave'
import { createLearningCategoryFormSchema, keyFromLabel } from './learningCategoryFormSchema'
import {
  forgetNewLearningCategoryDraft,
  readNewLearningCategoryDraft,
  saveNewLearningCategoryDraft,
} from './newLearningCategoryDraft'
import { toast } from '../../../toast/toast'

type Props = {
  // The workspace category being edited, or null to add one.
  category: LearningCategory | null
  onSaved: (category: LearningCategory) => void
}

// Adds one of the workspace's own categories, or edits one. A new category's key follows its
// name until it is typed by hand; once saved, the key never changes, since learnings store it.
// While new, what is typed is kept in session storage until it is saved.
export function LearningCategoryForm(props: Props) {
  const { t } = useTranslation([ 'learningCategories', 'common' ])
  const dispatch = useAppDispatch()
  const stored = props.category
  const labelRef = useRef<HTMLInputElement>(null)
  const [ isKeyTyped, setIsKeyTyped ] = useState(false)

  const resolver = useMemo(() => zodResolver(createLearningCategoryFormSchema(t)), [ t ])
  const form = useForm<LearningCategoryFormInput, unknown, LearningCategoryFormValues>({
    resolver,
    defaultValues: {
      label: stored?.label ?? '',
      key: stored?.key ?? '',
      description: stored?.description ?? '',
    },
  })

  useEffect(() => {
    labelRef.current?.focus()
  }, [])

  // A new category's draft comes back as changes to the blank form, so Save is ready for it.
  useEffect(() => {
    if (stored) {
      return
    }
    const draft = readNewLearningCategoryDraft()
    if (!draft) {
      return
    }
    form.setValue('label', draft.label, { shouldDirty: true })
    form.setValue('key', draft.key, { shouldDirty: true })
    form.setValue('description', draft.description, { shouldDirty: true })
    setIsKeyTyped(draft.isKeyTyped)
  }, [ stored, form ])

  // useWatch subscribes per field and, unlike form.watch, is safe for the React Compiler.
  const values = useWatch({ control: form.control })

  useEffect(() => {
    if (stored) {
      return
    }
    saveNewLearningCategoryDraft({ ...values, isKeyTyped })
  }, [ stored, values, isKeyTyped ])

  const save = saveForm(form, async (submitted) => {
    try {
      if (stored) {
        const response = await updateLearningCategory(stored.key, {
          label: submitted.label,
          description: submitted.description,
        })
        dispatch(learningCategoryUpserted(response.category))
        // The form's new defaults are what was saved, so Save rests until the next change.
        form.reset(submitted)
        toast.success(t('toasts.saved'))
        props.onSaved(response.category)
        return true
      }

      const response = await createLearningCategory(submitted)
      dispatch(learningCategoryUpserted(response.category))
      forgetNewLearningCategoryDraft()
      toast.success(t('toasts.created'))
      props.onSaved(response.category)
      return true
    }
    catch (error) {
      if (!stored && error instanceof HTTPError && error.response.status === 409) {
        form.setError('key', { message: t('form.errors.keyTaken') })
        return false
      }
      // The form mirrors the API's rules, so a refusal here is one it did not foresee; its
      // message still names what to fix.
      const message = getApiErrorMessage(error)
      console.debug('LearningCategoryForm failed to save the category', { error })
      toast.danger(t('common:errors.unexpected'), { description: message ?? undefined })
      return false
    }
  })

  const onSubmit = usePageSave({
    save,
    isDirty: form.formState.isDirty,
  })

  const errors = form.formState.errors

  return <Form
    onSubmit={onSubmit}
    validationBehavior='aria'
    className='flex max-w-3xl flex-col gap-4'
  >
    {/* Name */}
    <TextField
      id='learning-category-label'
      isRequired
      isInvalid={Boolean(errors.label)}
      value={values.label}
      onChange={(label) => {
        form.setValue('label', label, { shouldDirty: true, shouldValidate: true })
        if (!stored && !isKeyTyped) {
          form.setValue('key', keyFromLabel(label), { shouldDirty: true, shouldValidate: Boolean(errors.key) })
        }
      }}
    >
      <Label>{t('form.label')}</Label>
      <Input ref={labelRef} autoComplete='off' />
      <FieldError>{errors.label?.message}</FieldError>
    </TextField>

    {/* Key */}
    <TextField
      id='learning-category-key'
      isRequired
      isReadOnly={Boolean(stored)}
      isInvalid={Boolean(errors.key)}
      value={values.key}
      onChange={(key) => {
        setIsKeyTyped(true)
        form.setValue('key', key, { shouldDirty: true, shouldValidate: true })
      }}
    >
      <Label>{t('form.key')}</Label>
      <Input autoComplete='off' spellCheck={false} className='font-mono' />
      <Description>{
        stored
          ? t('form.keyFixed')
          : t('form.keyHint')
      }</Description>
      <FieldError>{errors.key?.message}</FieldError>
    </TextField>

    {/* Description */}
    <TextField
      id='learning-category-description'
      isRequired
      isInvalid={Boolean(errors.description)}
      value={values.description}
      onChange={(description) => form.setValue('description', description, {
        shouldDirty: true,
        shouldValidate: true,
      })}
    >
      <Label>{t('form.description')}</Label>
      <TextArea rows={3} />
      <Description>{t('form.descriptionHint')}</Description>
      <FieldError>{errors.description?.message}</FieldError>
    </TextField>
  </Form>
}
