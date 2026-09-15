// Copyright © 2026 Jalapeno Labs

import type { Project } from '../../api/routes/projectRoutes'
import type { CoverChange } from './ProjectCoverField'
import type { ProjectFormValues } from './projectFormSchema'

// Core
import { useMemo, useState } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { projectUpserted } from '../../store/projectsSlice'

// User interface
import { Button, FieldError, Form, Input, Label, TextArea, TextField, toast } from '@heroui/react'
import { ProjectCoverField } from './ProjectCoverField'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { HTTPError } from 'ky'

// Misc
import {
  createProject,
  deleteProjectCover,
  getProjectCoverUrl,
  updateProject,
  uploadProjectCover,
} from '../../api/routes/projectRoutes'
import { createProjectFormSchema } from './projectFormSchema'

// `stacked` puts the cover under the other fields, for a narrow dialog. `split` puts it
// beside them on wide screens, at about the width it has on a tile.
const layoutClassNames = {
  stacked: 'flex flex-col gap-4',
  split: 'grid grid-cols-1 items-start gap-x-8 gap-y-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,22rem)]',
} as const satisfies Record<string, string>

type Props = {
  // The project being edited, or null to create one.
  project: Project | null
  layout: keyof typeof layoutClassNames
  onSaved: (project: Project) => void
  onCancel: () => void
}

// Name, description, and cover, for both the new project page and the edit dialog.
export function ProjectForm(props: Props) {
  const { t } = useTranslation([ 'projects', 'common' ])
  const dispatch = useAppDispatch()
  const [ coverChange, setCoverChange ] = useState<CoverChange>({ kind: 'keep' })

  const resolver = useMemo(
    () => zodResolver(createProjectFormSchema(t)),
    [ t ],
  )

  const form = useForm<ProjectFormValues>({
    resolver,
    defaultValues: {
      name: props.project?.name ?? '',
      description: props.project?.description ?? '',
    },
  })

  // The project is saved first, then its cover: a new project has no id until saved.
  // A cover that fails leaves the saved project in place and says so.
  async function applyCoverChange(project: Project) {
    if (coverChange.kind === 'keep') {
      return project
    }
    try {
      const response = coverChange.kind === 'replace'
        ? await uploadProjectCover(project.id, coverChange.file)
        : await deleteProjectCover(project.id)
      dispatch(projectUpserted(response.project))
      return response.project
    }
    catch (error) {
      console.debug('ProjectForm saved the project but not its cover', { error, projectId: project.id })
      toast.danger(t('toasts.coverFailed', { name: project.name }))
      return project
    }
  }

  const onSubmit = form.handleSubmit(async (values) => {
    try {
      const response = props.project
        ? await updateProject(props.project.id, values)
        : await createProject(values)
      dispatch(projectUpserted(response.project))
      const saved = await applyCoverChange(response.project)

      toast.success(
        props.project
          ? t('toasts.updated', { name: values.name })
          : t('toasts.created', { name: values.name }),
      )
      props.onSaved(saved)
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 409) {
        form.setError('name', { message: t('form.errors.nameTaken') })
        return
      }

      console.debug('ProjectForm failed to save the project', { error })
      toast.danger(t('common:errors.unexpected'))
    }
  })

  const errors = form.formState.errors
  // useWatch subscribes per field and, unlike form.watch, is safe for the React Compiler.
  const [ name, description ] = useWatch({
    control: form.control,
    name: [ 'name', 'description' ],
  })

  return <Form onSubmit={onSubmit} validationBehavior='aria' className='flex flex-col gap-4'>
    <div className={layoutClassNames[props.layout]}>
      <div className='flex flex-col gap-4'>
        {/* Name */}
        <TextField
          autoFocus
          isRequired
          isInvalid={Boolean(errors.name)}
          value={name}
          onChange={(value) => form.setValue('name', value, { shouldDirty: true, shouldValidate: true })}
        >
          <Label>{t('form.name')}</Label>
          <Input />
          <FieldError>{errors.name?.message}</FieldError>
        </TextField>

        {/* Description */}
        <TextField
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
        savedUrl={props.project
          ? getProjectCoverUrl(props.project)
          : null}
        name={name}
        change={coverChange}
        onChange={setCoverChange}
      />
    </div>

    <div className='mt-2 flex justify-end gap-2'>
      <Button variant='tertiary' onPress={props.onCancel}>
        <span>{t('common:actions.cancel')}</span>
      </Button>
      <Button
        type='submit'
        isPending={form.formState.isSubmitting}
      >
        <span>{
          props.project
            ? t('common:actions.save')
            : t('form.create')
        }</span>
      </Button>
    </div>
  </Form>
}
