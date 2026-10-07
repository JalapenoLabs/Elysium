// Copyright © 2026 Jalapeno Labs

import type { StudioFilters } from './studioListing'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectAllProjects } from '../../store/projectsSlice'

// User interface
import { Label, Switch } from '@heroui/react'
import { OptionSelect } from '../../components/OptionSelect'

// Misc
import { useProjectsLoader } from '../../hooks/useServerData'
import { NO_PROJECT } from './studioListing'

// Any project, as the select's key; the filters themselves hold null.
const ANY_PROJECT = 'any'

type Props = {
  filters: StudioFilters
  onChange: (filters: StudioFilters) => void
}

// The grid's filters: a project (or none), and whether to show deleted items instead.
export function StudioFiltersBar(props: Props) {
  const { t } = useTranslation('studio')
  useProjectsLoader()
  const projects = useAppSelector(selectAllProjects)
  const filters = props.filters

  const projectOptions = [
    { id: ANY_PROJECT, label: t('filters.anyProject') },
    { id: NO_PROJECT, label: t('filters.noProject') },
    ...projects.map((project) => ({ id: project.id, label: project.name })),
  ]

  return <div className='relaxed flex flex-wrap items-end gap-3'>
    <OptionSelect
      className='w-56'
      label={t('filters.project')}
      options={projectOptions}
      value={filters.project ?? ANY_PROJECT}
      onChange={(value) => props.onChange({
        ...filters,
        project: value === ANY_PROJECT
          ? null
          : value,
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
