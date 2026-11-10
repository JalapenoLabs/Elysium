// Copyright © 2026 Jalapeno Labs

import type { Project, UpdateProjectRequest } from '../../api/routes/projectRoutes'
import type { ProjectEditValues } from './projectFormSchema'

// Core
import { useEffect, useMemo, useState } from 'react'
import { useForm } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import { mutate } from 'swr'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { projectUpserted } from '../../store/projectsSlice'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { HTTPError } from 'ky'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import {
  getProjectAutonomyKey,
  setProjectAutonomy,
  updateProject,
  uploadProjectCover,
} from '../../api/routes/projectRoutes'
import { saveForm } from '../../layout/page/pageSave'
import { fromGithubKey, toGithubKey } from './projectGithub'
import { createProjectEditSchema } from './projectFormSchema'
import { toast } from '../../toast/toast'

function toFormValues(project: Project): ProjectEditValues {
  return {
    name: project.name,
    description: project.description,
    coverFit: project.coverFit,
    githubKey: toGithubKey(project.github),
    autonomy: project.autonomy,
  }
}

// The project page's form: its name, description, how its cover fits, its GitHub token, its level
// of autonomy, and a new cover picked to replace the old, all kept by the page's Save. Fields the person has
// not touched follow the project as the event stream changes it. The cover is a file rather
// than a value, so it is held beside the form and uploaded after the fields are saved; one
// that fails stays picked, so the next Save uploads it alone.
export function useProjectEditor(project: Project) {
  const { t } = useTranslation([ 'projects', 'common' ])
  const dispatch = useAppDispatch()
  const [ coverFile, setCoverFile ] = useState<File | null>(null)
  const resolver = useMemo(() => zodResolver(createProjectEditSchema(t)), [ t ])
  const form = useForm<ProjectEditValues>({
    resolver,
    defaultValues: toFormValues(project),
  })

  useEffect(() => {
    form.reset(toFormValues(project), { keepDirtyValues: true, keepDirty: true })
  }, [ form, project ])

  const save = saveForm(form, async (values) => {
    const fields: UpdateProjectRequest = {}
    if (values.name !== project.name) {
      fields.name = values.name
    }
    if (values.description !== project.description) {
      fields.description = values.description
    }
    if (values.coverFit !== project.coverFit) {
      fields.coverFit = values.coverFit
    }
    if (values.githubKey !== toGithubKey(project.github)) {
      fields.github = fromGithubKey(values.githubKey)
    }

    const turnsTheDial = values.autonomy !== project.autonomy

    let saved = project
    try {
      if (Object.keys(fields).length) {
        saved = (await updateProject(project.id, fields)).project
        dispatch(projectUpserted(saved))
      }
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 409) {
        toast.danger(t('form.errors.nameTaken'))
        return false
      }
      console.debug('useProjectEditor failed to save the project', { error, projectId: project.id })
      toast.danger(t('common:errors.unexpected'), { description: getApiErrorMessage(error) })
      return false
    }

    // The dial has a route of its own, which only a member may call and which records who turned
    // it. A refusal keeps the choice in the form, so the next Save sends it alone.
    if (turnsTheDial) {
      try {
        const response = await setProjectAutonomy(project.id, values.autonomy)
        saved = response.project
        dispatch(projectUpserted(saved))
        void mutate(getProjectAutonomyKey(project.id), { autonomy: response.autonomy }, { revalidate: false })
      }
      catch (error) {
        console.debug('useProjectEditor failed to set the level of autonomy', { error, projectId: project.id })
        toast.danger(t('autonomy.saveFailed'), { description: getApiErrorMessage(error) })
        return false
      }
    }

    if (saved !== project) {
      form.reset(toFormValues(saved))
    }

    if (coverFile) {
      try {
        saved = (await uploadProjectCover(project.id, coverFile)).project
        dispatch(projectUpserted(saved))
        setCoverFile(null)
      }
      catch (error) {
        // The API explains a refusal, such as a file that is not really an image.
        const description = error instanceof HTTPError && error.response.status === 400
          ? t('form.errors.coverUnsupported')
          : t('common:errors.unexpected')
        console.debug('useProjectEditor failed to upload the cover', { error, projectId: project.id })
        toast.danger(t('toasts.coverUploadFailed'), { description })
        return false
      }
    }

    toast.success(t('toasts.updated', { name: saved.name }))
    return true
  })

  return {
    form,
    save,
    coverFile,
    setCoverFile,
    isDirty: form.formState.isDirty || coverFile !== null,
  } as const
}

export type ProjectEditor = ReturnType<typeof useProjectEditor>
