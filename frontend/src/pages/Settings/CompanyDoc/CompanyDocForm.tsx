// Copyright © 2026 Jalapeno Labs

import type { CompanyDoc, CompanyDocSectionKey } from '../../../api/routes/companyDocRoutes'

// Core
import { useEffect, useMemo } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { companyDocLoaded } from '../../../store/companyDocSlice'
import { useAppDispatch } from '../../../store/hooks'

// User interface
import { Description, FieldError, Form, Label, TextArea, TextField } from '@heroui/react'
import { ProposalCard } from './ProposalCard'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { z } from 'zod'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import {
  COMPANY_DOC_SECTION_MAX_CHARACTERS,
  COMPANY_DOC_SECTIONS,
  editCompanyDoc,
} from '../../../api/routes/companyDocRoutes'
import { useInitialFocus } from '../../../hooks/useInitialFocus'
import { saveForm, usePageSave } from '../../../layout/page/pageSave'
import { sectionHintKeys, sectionTitleKeys } from './companyDocPresentation'
import { toast } from '../../../toast/toast'

type Props = {
  doc: CompanyDoc
}

type SectionValues = Record<CompanyDocSectionKey, string>

function contentsOf(doc: CompanyDoc): SectionValues {
  const values = {} as SectionValues
  for (const section of doc.sections) {
    values[section.section] = section.content
  }
  return values
}

// The doc's seven sections as one form, saved by the page's Save, each with the changes proposed
// to it beside it. A section the person has not touched follows the doc as it changes, so an
// approval made here or in another tab shows at once; one they edited keeps their text, and
// its proposals wait for that edit to be saved or discarded before they can be approved.
export function CompanyDocForm(props: Props) {
  const { t } = useTranslation([ 'companyDoc', 'common' ])
  const dispatch = useAppDispatch()
  const stored = useMemo(() => contentsOf(props.doc), [ props.doc ])

  const resolver = useMemo(() => {
    const section = z
      .string()
      .max(COMPANY_DOC_SECTION_MAX_CHARACTERS, t('form.tooLong', { max: COMPANY_DOC_SECTION_MAX_CHARACTERS }))
    const shape = {} as Record<CompanyDocSectionKey, typeof section>
    for (const key of COMPANY_DOC_SECTIONS) {
      shape[key] = section
    }
    return zodResolver(z.object(shape))
  }, [ t ])
  const form = useForm<SectionValues>({ resolver, defaultValues: stored })
  const firstFieldRef = useInitialFocus()
  const values = useWatch({ control: form.control })
  const dirtyFields = form.formState.dirtyFields

  // The doc as it now stands becomes the defaults; a section the person edited keeps their text.
  useEffect(() => {
    form.reset(stored, { keepDirtyValues: true, keepDirty: true })
  }, [ stored, form ])

  const save = saveForm(form, async (submitted) => {
    const changed: Partial<SectionValues> = {}
    for (const section of COMPANY_DOC_SECTIONS) {
      if (submitted[section] !== stored[section]) {
        changed[section] = submitted[section]
      }
    }
    try {
      const response = await editCompanyDoc(changed)
      dispatch(companyDocLoaded(response))
      // The new defaults are what was saved, trimmed as the API stores it, so Save rests.
      form.reset(contentsOf(response))
      toast.success(t('toasts.saved'))
      return true
    }
    catch (error) {
      console.debug('CompanyDocForm could not save the doc', { error })
      toast.danger(t('common:errors.unexpected'), { description: getApiErrorMessage(error) ?? undefined })
      return false
    }
  })
  const onSubmit = usePageSave({ save, isDirty: form.formState.isDirty })
  const errors = form.formState.errors

  return <Form onSubmit={onSubmit} validationBehavior='aria' className='flex flex-col gap-8'>{
    props.doc.sections.map((section, index) => {
      const key = section.section
      return <section
        key={key}
        className='grid items-start gap-4 lg:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]'
      >
        <TextField
          id={`company-doc-${key}`}
          isInvalid={Boolean(errors[key])}
          value={values[key] ?? ''}
          onChange={(value) => form.setValue(key, value, { shouldDirty: true, shouldValidate: true })}
        >
          <Label>{t(sectionTitleKeys[key])}</Label>
          <TextArea
            ref={index === 0
              ? firstFieldRef
              : undefined}
            rows={6}
            autoComplete='off'
          />
          <Description>{t(sectionHintKeys[key])}</Description>
          <FieldError>{errors[key]?.message}</FieldError>
        </TextField>
        {section.proposals.length > 0 && <ul
          aria-label={t('proposal.listLabel', { section: t(sectionTitleKeys[key]) })}
          className='flex flex-col gap-3'
        >{
            section.proposals.map((proposal) => <li key={proposal.id}>
              <ProposalCard
                proposal={proposal}
                currentContent={section.content}
                hasUnsavedEdits={Boolean(dirtyFields[key])}
              />
            </li>)
          }</ul>}
      </section>
    })
  }</Form>
}
