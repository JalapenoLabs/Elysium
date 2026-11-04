// Copyright © 2026 Jalapeno Labs

import type { AgentModel, ModelChoice } from '../../../api/routes/modelSettingRoutes'
import type { ModelSettingRow } from './ModelSettingsTable'

// Core
import { useCallback, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsAdmin } from '../../../store/authSlice'
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import {
  agentModelUpserted,
  selectAgentModels,
  selectEfforts,
  selectSuggestedModels,
} from '../../../store/modelSettingsSlice'

// User interface
import { Spinner } from '@heroui/react'
import { EmptyNotice } from '../../../components/EmptyNotice'
import { SettingsPage } from '../SettingsPage'
import { ModelSettingsTable } from './ModelSettingsTable'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { resetModelSetting, setModelSetting } from '../../../api/routes/modelSettingRoutes'
import { useModelSettingsLoader } from '../../../hooks/useServerData'
import { toast } from '../../../toast/toast'
import { nameFromIdentifier } from '../../Runs/runPresentation'
import { useRunNames } from '../../Runs/useRunNames'
import {
  agentNameKeys,
  currentChoice,
  isModelName,
  pendingChanges,
} from './modelSettingsPresentation'

// `/settings/models`: the model and effort each workflow step and agent runs on. Admins only;
// anyone else who opens the address is told so, and the settings directory hides the entry.
//
// Each row's model and effort are the page's fields, kept by its Save. Only what is changed
// here is held and sent, so a change another admin makes meanwhile shows at once and is never
// undone. Use default acts at once.
export function ModelSettingsPage() {
  const { t } = useTranslation([ 'models', 'common' ])
  const dispatch = useAppDispatch()
  const isAdmin = useAppSelector(selectIsAdmin)
  const status = useModelSettingsLoader(isAdmin)
  const agents = useAppSelector(selectAgentModels)
  const suggestedModels = useAppSelector(selectSuggestedModels)
  const efforts = useAppSelector(selectEfforts)
  const runNames = useRunNames()
  const [ changes, setChanges ] = useState<Record<string, ModelChoice>>({})

  const nameOf = useCallback((agent: AgentModel) => {
    if (agent.kind === 'step') {
      return runNames.step(agent.name)
    }
    const key = agentNameKeys.get(agent.name)
    return key
      ? t(key)
      : nameFromIdentifier(agent.name)
  }, [ runNames, t ])

  const rows = useMemo<ModelSettingRow[]>(
    () => agents.map((agent) => ({
      agent,
      name: nameOf(agent),
      shown: changes[agent.name] ?? currentChoice(agent),
    })),
    [ agents, changes, nameOf ],
  )

  const pending = pendingChanges(agents, changes)
  const invalid = pending.find((change) => !isModelName(change.choice.model.trim()))

  async function save() {
    // A change the API refuses stays picked so it can be fixed.
    const refused: Record<string, ModelChoice> = {}
    for (const change of pending) {
      try {
        const response = await setModelSetting(change.agent.name, {
          model: change.choice.model.trim(),
          effort: change.choice.effort,
        })
        dispatch(agentModelUpserted(response.agent))
      }
      catch (error) {
        console.debug('ModelSettingsPage could not save a model setting', { error, agent: change.agent.name })
        toast.danger(t('toasts.saveFailed', { name: nameOf(change.agent) }), {
          description: getApiErrorMessage(error) ?? undefined,
        })
        refused[change.agent.name] = change.choice
      }
    }
    setChanges(refused)
    const isSaved = !Object.keys(refused).length
    if (isSaved) {
      toast.success(t('toasts.saved'))
    }
    return isSaved
  }

  const pageSave = {
    save,
    isDirty: pending.length > 0,
    blockedReason: invalid
      ? t('invalidModelFor', { name: nameOf(invalid.agent) })
      : null,
  }

  const onChange = useCallback((agent: AgentModel, choice: ModelChoice) => {
    setChanges((previous) => ({ ...previous, [agent.name]: choice }))
  }, [])

  const onUseDefault = useCallback(async (agent: AgentModel) => {
    try {
      const response = await resetModelSetting(agent.name)
      dispatch(agentModelUpserted(response.agent))
      // What was picked for it here is dropped with its setting.
      setChanges((previous) => {
        const rest = { ...previous }
        delete rest[agent.name]
        return rest
      })
      toast.success(t('toasts.reset', { name: nameOf(agent) }))
    }
    catch (error) {
      console.debug('ModelSettingsPage could not put an agent back on its default', { error, agent: agent.name })
      toast.danger(t('common:errors.unexpected'), { description: getApiErrorMessage(error) ?? undefined })
    }
  }, [ dispatch, nameOf, t ])

  if (!isAdmin) {
    return <SettingsPage title={t('title')}>
      <EmptyNotice>{t('adminOnly')}</EmptyNotice>
    </SettingsPage>
  }

  return <SettingsPage
    title={t('title')}
    help={t('help')}
    save={pageSave}
  >
    {status === 'loading' && <div className='grid place-items-center py-16'>
      <Spinner />
    </div>}

    {status === 'failed' && <p className='py-10 text-center text-sm text-danger'>{
      t('table.loadError')
    }</p>}

    {status === 'loaded' && <ModelSettingsTable
      rows={rows}
      suggestedModels={suggestedModels}
      efforts={efforts}
      onChange={onChange}
      onUseDefault={onUseDefault}
    />}
  </SettingsPage>
}
