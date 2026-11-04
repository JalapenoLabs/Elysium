// Copyright © 2026 Jalapeno Labs

import type {
  AgentKind,
  ModelChoice,
  ProjectKindModel,
} from '../../api/routes/modelSettingRoutes'
import type { Project } from '../../api/routes/projectRoutes'
import type { ProjectKindModelRow } from './ProjectModelsTable'

// Core
import { useCallback, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { Spinner } from '@heroui/react'
import { Page } from '../../layout/page/Page'
import { ProjectModelsTable } from './ProjectModelsTable'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import {
  getProjectModelSettingsKey,
  listProjectModelSettings,
  resetProjectModelSetting,
  setProjectModelSetting,
} from '../../api/routes/modelSettingRoutes'
import { toast } from '../../toast/toast'
import { getProjectViewUrl, UrlTree } from '../../urls'
import { isModelName, kindLabelKeys, pendingChanges } from '../Settings/Models/modelSettingsPresentation'

type Props = {
  project: Project
}

// What `kind` runs on for the project now: its own choice, or what it follows.
function currentChoice(kind: ProjectKindModel): ModelChoice {
  const chosen = kind.setting ?? kind.workspace
  return {
    model: chosen.model,
    effort: chosen.effort,
  }
}

// The models one project's own agents run on: a row per kind that works for one project, each
// following the workspace until the project chooses. Models and efforts are the page's fields,
// kept by its one Save; Use workspace setting acts at once.
//
// The list is read through SWR alone, as an initiative's burnup is: no other view shows it, and
// `kindModelSetting.changed` revalidates its key while the page holds it.
export function ProjectModels(props: Props) {
  const { t } = useTranslation([ 'models', 'projects', 'common' ])
  const project = props.project
  const key = getProjectModelSettingsKey(project.id)
  const listing = useSWR(key, () => listProjectModelSettings(project.id))
  const [ changes, setChanges ] = useState<Partial<Record<AgentKind, ModelChoice>>>({})

  const kinds = useMemo(() => listing.data?.kinds ?? [], [ listing.data ])
  const nameOf = useCallback((kind: ProjectKindModel) => t(kindLabelKeys[kind.name]), [ t ])

  const rows = useMemo<ProjectKindModelRow[]>(
    () => kinds.map((kind) => ({
      kind,
      name: nameOf(kind),
      shown: changes[kind.name] ?? currentChoice(kind),
    })),
    [ kinds, changes, nameOf ],
  )

  const pending = pendingChanges(kinds, changes, currentChoice)
  const invalid = pending.find((change) => !isModelName(change.choice.model.trim()))

  async function save() {
    // A change the API refuses stays picked so it can be fixed.
    const refused: Partial<Record<AgentKind, ModelChoice>> = {}
    for (const change of pending) {
      try {
        await setProjectModelSetting(project.id, change.row.name, {
          model: change.choice.model.trim(),
          effort: change.choice.effort,
        })
      }
      catch (error) {
        console.debug('ProjectModels could not save a kind', { error, kind: change.row.name })
        toast.danger(t('toasts.saveFailed', { name: nameOf(change.row) }), {
          description: getApiErrorMessage(error) ?? undefined,
        })
        refused[change.row.name] = change.choice
      }
    }
    await listing.mutate()
    setChanges(refused)
    const isSaved = !Object.keys(refused).length
    if (isSaved) {
      toast.success(t('project.saved'))
    }
    return isSaved
  }

  const onChange = useCallback((kind: ProjectKindModel, choice: ModelChoice) => {
    setChanges((previous) => ({ ...previous, [kind.name]: choice }))
  }, [])

  const mutateListing = listing.mutate
  const onUseWorkspace = useCallback(async (kind: ProjectKindModel) => {
    try {
      await resetProjectModelSetting(project.id, kind.name)
      await mutateListing()
      setChanges((previous) => {
        const rest = { ...previous }
        delete rest[kind.name]
        return rest
      })
      toast.success(t('project.followsWorkspace', { name: nameOf(kind) }))
    }
    catch (error) {
      console.debug('ProjectModels could not let a kind follow the workspace', { error, kind: kind.name })
      toast.danger(t('common:errors.unexpected'), { description: getApiErrorMessage(error) ?? undefined })
    }
  }, [ project.id, mutateListing, nameOf, t ])

  return <Page
    title={t('project.title')}
    help={t('project.help')}
    parents={[
      { label: t('projects:title'), href: UrlTree.projects },
      { label: project.name, href: getProjectViewUrl(project.id) },
    ]}
    save={{
      save,
      isDirty: pending.length > 0,
      blockedReason: invalid
        ? t('invalidModelFor', { name: nameOf(invalid.row) })
        : null,
    }}
  >
    {!listing.data && !listing.error && <div className='grid place-items-center py-16'>
      <Spinner />
    </div>}

    {!listing.data && listing.error && <p className='py-10 text-center text-sm text-danger'>{
      t('project.loadError')
    }</p>}

    {listing.data && <ProjectModelsTable
      rows={rows}
      choices={listing.data.choices}
      usageSince={listing.data.usageSince}
      onChange={onChange}
      onUseWorkspace={onUseWorkspace}
    />}
  </Page>
}
