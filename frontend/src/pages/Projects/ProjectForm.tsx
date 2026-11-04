// Copyright © 2026 Jalapeno Labs

import type { Project } from '../../api/routes/projectRoutes'
import type { ProjectFormValues } from './projectFormSchema'

// Core
import { useMemo, useState } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { projectUpserted } from '../../store/projectsSlice'

// User interface
import { FieldError, Form, Input, Label, TextArea, TextField } from '@heroui/react'
import { PageCreateFooter } from '../../layout/page/PageCreateFooter'
import { ProjectCoverField } from './ProjectCoverField'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { HTTPError } from 'ky'

// Misc
import { createProject, uploadProjectCover } from '../../api/routes/projectRoutes'
import { useInitialFocus } from '../../hooks/useInitialFocus'
import { saveForm, usePageSave } from '../../layout/page/pageSave'
import { createProjectFormSchema } from './projectFormSchema'
import { toast } from '../../toast/toast'

type Props = {
  onSaved: (project: Project) => void
}

// Name, description, and cover for a new project, created from the header's Save or the
// Create button below the form: two places, one action. An existing project is edited in
// place on its own page.
export function ProjectForm(props: Props) {
  const { t } = useTranslation([ 'projects', 'common' ])
  const dispatch = useAppDispatch()
  const [ coverFile, setCoverFile ] = useState<File | null>(null)
  const nameRef = useInitialFocus()

  const resolver = useMemo(
    () => zodResolver(createProjectFormSchema(t)),
    [ t ],
  )

  const form = useForm<ProjectFormValues>({
    resolver,
    defaultValues: {
      name: '',
      description: '',
    },
  })

  // The project is saved first, then its cover: a new project has no id until saved.
  // A cover that fails leaves the saved project in place and says so.
  async function uploadCover(project: Project) {
    if (!coverFile) {
      return project
    }
    try {
      const response = await uploadProjectCover(project.id, coverFile)
      dispatch(projectUpserted(response.project))
      return response.project
    }
    catch (error) {
      console.debug('ProjectForm saved the project but not its cover', { error, projectId: project.id })
      toast.danger(t('toasts.coverFailed', { name: project.name }))
      return project
    }
  }

  const save = saveForm(form, async (values) => {
    try {
      const response = await createProject(values)
      dispatch(projectUpserted(response.project))
      const saved = await uploadCover(response.project)

      toast.success(t('toasts.created', { name: values.name }))
      props.onSaved(saved)
      return true
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 409) {
        form.setError('name', { message: t('form.errors.nameTaken') })
        return false
      }

      console.debug('ProjectForm failed to save the project', { error })
      toast.danger(t('common:errors.unexpected'))
      return false
    }
  })

  const onSubmit = usePageSave({
    save,
    // A cover picked before anything is typed is work to keep too.
    isDirty: form.formState.isDirty || coverFile !== null,
    label: t('form.create'),
  })

  const errors = form.formState.errors
  // useWatch subscribes per field and, unlike form.watch, is safe for the React Compiler.
  const [ name, description ] = useWatch({
    control: form.control,
    name: [ 'name', 'description' ],
  })

  return <Form onSubmit={onSubmit} validationBehavior='aria' className='flex flex-col gap-4'>
    {/* The cover sits beside the other fields, at about the width it has on a tile. */}
    <div className='grid grid-cols-1 items-start gap-x-8 gap-y-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,22rem)]'>
      <div className='flex flex-col gap-4'>
        {/* Name */}
        <TextField
          isRequired
          id='project-name'
          autoComplete='off'
          isInvalid={Boolean(errors.name)}
          value={name}
          onChange={(value) => form.setValue('name', value, { shouldDirty: true, shouldValidate: true })}
        >
          <Label>{t('form.name')}</Label>
          <Input ref={nameRef} />
          <FieldError>{errors.name?.message}</FieldError>
        </TextField>

        {/* Description */}
        <TextField
          id='project-description'
          isInvalid={Boolean(errors.description)}
          value={description}
          onChange={(value) => form.setValue('description', value, { shouldDirty: true, shouldValidate: true })}
        >
          <Label>{t('form.description')}</Label>
          <TextArea rows={3} />
          <FieldError>{errors.description?.message}</FieldError>
        </TextField>
      </div>

      {/* Cover image */}
      <ProjectCoverField
        name={name}
        file={coverFile}
        onChange={setCoverFile}
      />
    </div>

    <PageCreateFooter />
  </Form>
}
