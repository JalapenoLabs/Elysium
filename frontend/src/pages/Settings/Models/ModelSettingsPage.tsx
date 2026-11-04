// Copyright © 2026 Jalapeno Labs

import type {
  AgentKind,
  AgentModel,
  KindModel,
  ModelChoice,
} from '../../../api/routes/modelSettingRoutes'
import type { AgentModelRow } from './AgentModelsTable'
import type { KindModelRow } from './KindModelsTable'

// Core
import { useCallback, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsAdmin } from '../../../store/authSlice'
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import {
  agentModelUpserted,
  kindModelUpserted,
  selectAgentModels,
  selectKindModels,
  selectModelChoices,
  selectUsageSince,
} from '../../../store/modelSettingsSlice'

// User interface
import { Spinner, Tabs } from '@heroui/react'
import { EmptyNotice } from '../../../components/EmptyNotice'
import { PageTabs } from '../../../components/PageTabs'
import { SettingsPage } from '../SettingsPage'
import { AgentModelsTable } from './AgentModelsTable'
import { KindModelsTable } from './KindModelsTable'

// Utility
import { mutate } from 'swr'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import {
  MODEL_SETTINGS_KEY,
  resetKindModelSetting,
  resetModelSetting,
  setKindModelSetting,
  setModelSetting,
} from '../../../api/routes/modelSettingRoutes'
import { listFormat } from '../../../format/intl'
import { useModelSettingsLoader } from '../../../hooks/useServerData'
import { useTabParam } from '../../../hooks/useTabParam'
import { toast } from '../../../toast/toast'
import { nameFromIdentifier } from '../../Runs/runPresentation'
import { useRunNames } from '../../Runs/useRunNames'
import {
  agentNameKeys,
  currentAgentChoice,
  currentKindChoice,
  isModelName,
  kindLabelKeys,
  membersByKind,
  pendingChanges,
} from './modelSettingsPresentation'

const MODEL_TABS = [ 'kinds', 'agents' ] as const
type ModelTab = typeof MODEL_TABS[number]

const tabLabelKeys = {
  kinds: 'tabs.kinds',
  agents: 'tabs.agents',
} as const satisfies Record<ModelTab, string>

// `/settings/models`: the model and effort each kind of agent, and each workflow step and
// agent, runs on across the workspace. Admins only; anyone else who opens the address is told
// so, and the settings directory hides the entry.
//
// Kinds and steps and agents are a tab each, and their models and efforts are the page's
// fields, kept by its one Save. Only what is changed here is held and sent, so a change another
// admin makes meanwhile shows at once and is never undone. Use default and Follow kind act at
// once.
export function ModelSettingsPage() {
  const { t } = useTranslation([ 'models', 'common' ])
  const dispatch = useAppDispatch()
  const isAdmin = useAppSelector(selectIsAdmin)
  const status = useModelSettingsLoader(isAdmin)
  const kinds = useAppSelector(selectKindModels)
  const agents = useAppSelector(selectAgentModels)
  const choices = useAppSelector(selectModelChoices)
  const usageSince = useAppSelector(selectUsageSince)
  const runNames = useRunNames()
  const [ selectedTab, selectTab ] = useTabParam(MODEL_TABS)
  const [ kindChanges, setKindChanges ] = useState<Partial<Record<AgentKind, ModelChoice>>>({})
  const [ agentChanges, setAgentChanges ] = useState<Record<string, ModelChoice>>({})

  const nameOfAgent = useCallback((agent: AgentModel) => {
    if (agent.isStep) {
      return runNames.step(agent.name)
    }
    const key = agentNameKeys.get(agent.name)
    return key
      ? t(key)
      : nameFromIdentifier(agent.name)
  }, [ runNames, t ])

  const nameOfKind = useCallback((kind: KindModel) => t(kindLabelKeys[kind.name]), [ t ])

  const kindRows = useMemo<KindModelRow[]>(() => {
    const members = membersByKind(agents, nameOfAgent)
    return kinds.map((kind) => {
      const names = members.get(kind.name)
      return {
        kind,
        name: nameOfKind(kind),
        members: names
          ? listFormat({ type: 'conjunction' }).format(names)
          : null,
        shown: kindChanges[kind.name] ?? currentKindChoice(kind),
      }
    })
  }, [ kinds, agents, kindChanges, nameOfAgent, nameOfKind ])

  const agentRows = useMemo<AgentModelRow[]>(
    () => agents.map((agent) => ({
      agent,
      name: nameOfAgent(agent),
      shown: agentChanges[agent.name] ?? currentAgentChoice(agent),
    })),
    [ agents, agentChanges, nameOfAgent ],
  )

  const pendingKinds = pendingChanges(kinds, kindChanges, currentKindChoice)
  const pendingAgents = pendingChanges(agents, agentChanges, currentAgentChoice)
  const invalidKind = pendingKinds.find((change) => !isModelName(change.choice.model.trim()))
  const invalidAgent = pendingAgents.find((change) => !isModelName(change.choice.model.trim()))

  function blockedReason() {
    if (invalidKind) {
      return t('invalidModelFor', { name: nameOfKind(invalidKind.row) })
    }
    if (invalidAgent) {
      return t('invalidModelFor', { name: nameOfAgent(invalidAgent.row) })
    }
    return null
  }

  // Kinds go first, so a step or agent picked to match its kind's new choice is stored as
  // following it rather than as a setting of its own.
  async function save() {
    // A change the API refuses stays picked so it can be fixed.
    const refusedKinds: Partial<Record<AgentKind, ModelChoice>> = {}
    for (const change of pendingKinds) {
      try {
        const response = await setKindModelSetting(change.row.name, {
          model: change.choice.model.trim(),
          effort: change.choice.effort,
        })
        dispatch(kindModelUpserted(response.kind))
      }
      catch (error) {
        console.debug('ModelSettingsPage could not save a kind', { error, kind: change.row.name })
        toast.danger(t('toasts.saveFailed', { name: nameOfKind(change.row) }), {
          description: getApiErrorMessage(error) ?? undefined,
        })
        refusedKinds[change.row.name] = change.choice
      }
    }

    const refusedAgents: Record<string, ModelChoice> = {}
    for (const change of pendingAgents) {
      try {
        const response = await setModelSetting(change.row.name, {
          model: change.choice.model.trim(),
          effort: change.choice.effort,
        })
        dispatch(agentModelUpserted(response.agent))
      }
      catch (error) {
        console.debug('ModelSettingsPage could not save a model setting', { error, agent: change.row.name })
        toast.danger(t('toasts.saveFailed', { name: nameOfAgent(change.row) }), {
          description: getApiErrorMessage(error) ?? undefined,
        })
        refusedAgents[change.row.name] = change.choice
      }
    }

    setKindChanges(refusedKinds)
    setAgentChanges(refusedAgents)
    if (pendingKinds.length) {
      // What each step and agent resolves to may have moved with its kind.
      void mutate(MODEL_SETTINGS_KEY)
    }
    const isSaved = !Object.keys(refusedKinds).length && !Object.keys(refusedAgents).length
    if (isSaved) {
      toast.success(t('toasts.saved'))
    }
    return isSaved
  }

  const pageSave = {
    save,
    isDirty: pendingKinds.length > 0 || pendingAgents.length > 0,
    blockedReason: blockedReason(),
  }

  const onKindChange = useCallback((kind: KindModel, choice: ModelChoice) => {
    setKindChanges((previous) => ({ ...previous, [kind.name]: choice }))
  }, [])

  const onAgentChange = useCallback((agent: AgentModel, choice: ModelChoice) => {
    setAgentChanges((previous) => ({ ...previous, [agent.name]: choice }))
  }, [])

  const onUseKindDefault = useCallback(async (kind: KindModel) => {
    try {
      const response = await resetKindModelSetting(kind.name)
      dispatch(kindModelUpserted(response.kind))
      void mutate(MODEL_SETTINGS_KEY)
      // What was picked for it here is dropped with its setting.
      setKindChanges((previous) => {
        const rest = { ...previous }
        delete rest[kind.name]
        return rest
      })
      toast.success(t('toasts.reset', { name: nameOfKind(kind) }))
    }
    catch (error) {
      console.debug('ModelSettingsPage could not put a kind back on its default', { error, kind: kind.name })
      toast.danger(t('common:errors.unexpected'), { description: getApiErrorMessage(error) ?? undefined })
    }
  }, [ dispatch, nameOfKind, t ])

  const onFollowKind = useCallback(async (agent: AgentModel) => {
    try {
      const response = await resetModelSetting(agent.name)
      dispatch(agentModelUpserted(response.agent))
      setAgentChanges((previous) => {
        const rest = { ...previous }
        delete rest[agent.name]
        return rest
      })
      toast.success(t('toasts.followsKind', { name: nameOfAgent(agent) }))
    }
    catch (error) {
      console.debug('ModelSettingsPage could not let an agent follow its kind', { error, agent: agent.name })
      toast.danger(t('common:errors.unexpected'), { description: getApiErrorMessage(error) ?? undefined })
    }
  }, [ dispatch, nameOfAgent, t ])

  if (!isAdmin) {
    return <SettingsPage title={t('title')}>
      <EmptyNotice>{t('adminOnly')}</EmptyNotice>
    </SettingsPage>
  }

  const isLoaded = status === 'loaded'
  const countByTab = {
    kinds: isLoaded
      ? kinds.length
      : null,
    agents: isLoaded
      ? agents.length
      : null,
  } satisfies Record<ModelTab, number | null>

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

    {isLoaded && usageSince && <PageTabs
      label={t('tabs.label')}
      selectedKey={selectedTab}
      onSelectionChange={selectTab}
      tabs={MODEL_TABS.map((tab) => ({ id: tab, label: t(tabLabelKeys[tab]), count: countByTab[tab] }))}
    >
      <Tabs.Panel id='kinds'>
        <KindModelsTable
          rows={kindRows}
          choices={choices}
          usageSince={usageSince}
          onChange={onKindChange}
          onUseDefault={onUseKindDefault}
        />
      </Tabs.Panel>
      <Tabs.Panel id='agents'>
        <AgentModelsTable
          rows={agentRows}
          choices={choices}
          onChange={onAgentChange}
          onFollowKind={onFollowKind}
        />
      </Tabs.Panel>
    </PageTabs>}
  </SettingsPage>
}
