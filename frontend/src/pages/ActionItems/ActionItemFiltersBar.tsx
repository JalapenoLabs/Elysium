// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { ParseKeys } from 'i18next'
import type { ActionItemFilters, Tristate } from './actionItemFilters'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectLiveInitiatives } from '../../store/initiativesSlice'
import { selectAllProjects } from '../../store/projectsSlice'

// User interface
import { Label, ListBox, Select, Switch } from '@heroui/react'
import { OptionSelect } from '../../components/OptionSelect'

// Misc
import { ACTION_ITEM_STATES } from '../../api/routes/actionItemRoutes'
import { useInitiativesLoader, useProjectsLoader } from '../../hooks/useServerData'
import { NO_PROJECT, TRISTATE_CHOICES } from './actionItemFilters'
import { stateLabelKeys } from './actionItemPresentation'

// Any project or initiative, as a select's key; the filters themselves hold null.
const ANY = 'any'

const tristateLabelKeys = {
  any: 'filters.any',
  yes: 'filters.yes',
  no: 'filters.no',
} as const satisfies Record<Tristate, ParseKeys<'actionItems'>>

type Props = {
  filters: ActionItemFilters
  onChange: (filters: ActionItemFilters) => void
}

// One row of filters above the All items table: states, project, initiative, waiting,
// snoozed, and whether to show deleted items instead.
export function ActionItemFiltersBar(props: Props) {
  const { t } = useTranslation('actionItems')
  useProjectsLoader()
  useInitiativesLoader()
  const projects = useAppSelector(selectAllProjects)
  const initiatives = useAppSelector(selectLiveInitiatives)
  const filters = props.filters

  const tristateOptions = TRISTATE_CHOICES.map((choice) => ({ id: choice, label: t(tristateLabelKeys[choice]) }))
  const projectOptions = [
    { id: ANY, label: t('filters.anyProject') },
    { id: NO_PROJECT, label: t('filters.noProject') },
    ...projects.map((project) => ({ id: project.id, label: project.name })),
  ]
  const initiativeOptions = [
    { id: ANY, label: t('filters.anyInitiative') },
    ...initiatives.map((initiative) => ({ id: initiative.id, label: initiative.name })),
  ]

  return <div className='relaxed flex flex-wrap items-end gap-3'>
    <Select
      className='w-56'
      selectionMode='multiple'
      placeholder={t('filters.allStates')}
      value={filters.states}
      onChange={(keys: Key[]) => props.onChange({
        ...filters,
        states: ACTION_ITEM_STATES.filter((state) => keys.includes(state)),
      })}
    >
      <Label>{t('filters.state')}</Label>
      <Select.Trigger>
        <Select.Value />
        <Select.Indicator />
      </Select.Trigger>
      <Select.Popover>
        <ListBox selectionMode='multiple'>{
          ACTION_ITEM_STATES.map((state) => <ListBox.Item key={state} id={state} textValue={t(stateLabelKeys[state])}>
            {t(stateLabelKeys[state])}
            <ListBox.ItemIndicator />
          </ListBox.Item>)
        }</ListBox>
      </Select.Popover>
    </Select>

    <OptionSelect
      className='w-48'
      label={t('filters.project')}
      options={projectOptions}
      value={filters.project ?? ANY}
      onChange={(value) => props.onChange({
        ...filters,
        project: value === ANY
          ? null
          : value,
      })}
    />

    <OptionSelect
      className='w-48'
      label={t('filters.initiative')}
      options={initiativeOptions}
      value={filters.initiative ?? ANY}
      onChange={(value) => props.onChange({
        ...filters,
        initiative: value === ANY
          ? null
          : value,
      })}
    />

    <OptionSelect
      className='w-32'
      label={t('filters.waiting')}
      options={tristateOptions}
      value={filters.waiting}
      onChange={(value) => props.onChange({
        ...filters,
        waiting: TRISTATE_CHOICES.find((choice) => choice === value) ?? 'any',
      })}
    />

    <OptionSelect
      className='w-32'
      label={t('filters.snoozed')}
      options={tristateOptions}
      value={filters.snoozed}
      onChange={(value) => props.onChange({
        ...filters,
        snoozed: TRISTATE_CHOICES.find((choice) => choice === value) ?? 'any',
      })}
    />

    <Switch
      className='h-10 flex-row items-center gap-2'
      isSelected={filters.deleted}
      onChange={(isSelected) => props.onChange({ ...filters, deleted: isSelected })}
    >
      <Switch.Control>
        <Switch.Thumb />
      </Switch.Control>
      <Switch.Content>
        <Label>{t('filters.deleted')}</Label>
      </Switch.Content>
    </Switch>
  </div>
}
