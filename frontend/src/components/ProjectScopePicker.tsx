// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { ProjectScope } from '../api/routes/projectRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../store/hooks'
import { selectAllProjects } from '../store/projectsSlice'

// User interface
import {
  Autocomplete,
  Description,
  EmptyState,
  FieldError,
  Label,
  ListBox,
  SearchField,
  Tag,
  TagGroup,
  useFilter,
} from '@heroui/react'

// Misc
import { ALL_PROJECTS } from '../api/routes/projectRoutes'
import { useProjectsLoader } from '../hooks/useServerData'

type Props = {
  label: string
  description?: string
  value: ProjectScope
  onChange: (value: ProjectScope) => void
  errorMessage?: string
}

// Picks projects as removable tags from a searchable dropdown. `*` stands for every
// project, including ones added later, and replaces any projects picked one by one;
// picking a project while `*` is chosen replaces `*`.
export function ProjectScopePicker(props: Props) {
  const { t } = useTranslation('projects')
  useProjectsLoader()
  const projects = useAppSelector(selectAllProjects)
  const { contains } = useFilter({ sensitivity: 'base' })

  const selectedKeys = props.value === ALL_PROJECTS
    ? [ ALL_PROJECTS ]
    : props.value
  const allProjectsLabel = t('scope.all')

  function labelFor(key: string) {
    if (key === ALL_PROJECTS) {
      return allProjectsLabel
    }
    return projects.find((project) => project.id === key)?.name
  }

  function select(keys: string[]) {
    const wasAll = props.value === ALL_PROJECTS
    const wantsAll = keys.includes(ALL_PROJECTS)
    if (wantsAll && !wasAll) {
      props.onChange(ALL_PROJECTS)
      return
    }
    props.onChange(keys.filter((key) => key !== ALL_PROJECTS))
  }

  return <Autocomplete
    selectionMode='multiple'
    isInvalid={Boolean(props.errorMessage)}
    placeholder={t('scope.placeholder')}
    value={selectedKeys}
    onChange={(keys: Key | Key[] | null) => select(Array.isArray(keys)
      ? keys.map(String)
      : [])}
  >
    <Label>{props.label}</Label>
    <Autocomplete.Trigger>
      <Autocomplete.Value>{
        ({ defaultChildren, isPlaceholder }) => {
          if (isPlaceholder || !selectedKeys.length) {
            return defaultChildren
          }
          return <TagGroup
            size='sm'
            aria-label={props.label}
            onRemove={(removed) => select(selectedKeys.filter((key) => !removed.has(key)))}
          >
            <TagGroup.List>{
              selectedKeys.map((key) => {
                const name = labelFor(key)
                if (!name) {
                  console.debug('ProjectScopePicker has no project for a selected id', { key })
                  return null
                }
                return <Tag key={key} id={key}>{name}</Tag>
              })
            }</TagGroup.List>
          </TagGroup>
        }
      }</Autocomplete.Value>
      <Autocomplete.Indicator />
    </Autocomplete.Trigger>
    {props.description && <Description>{props.description}</Description>}
    <FieldError>{props.errorMessage}</FieldError>
    <Autocomplete.Popover>
      <Autocomplete.Filter filter={contains}>
        <SearchField autoFocus aria-label={t('scope.search')} variant='secondary'>
          <SearchField.Group>
            <SearchField.SearchIcon />
            <SearchField.Input placeholder={t('scope.search')} />
            <SearchField.ClearButton />
          </SearchField.Group>
        </SearchField>
        <ListBox renderEmptyState={() => <EmptyState>{t('scope.empty')}</EmptyState>}>
          {/* Typing * finds every project, as it is written in the tag input */}
          <ListBox.Item id={ALL_PROJECTS} textValue={`${ALL_PROJECTS} ${allProjectsLabel}`}>
            <span className='flex items-center gap-2'>
              <code className='text-xs opacity-70'>{ALL_PROJECTS}</code>
              <span>{allProjectsLabel}</span>
            </span>
            <ListBox.ItemIndicator />
          </ListBox.Item>
          {projects.map((project) => <ListBox.Item
            key={project.id}
            id={project.id}
            textValue={project.name}
          >
            {project.name}
            <ListBox.ItemIndicator />
          </ListBox.Item>)}
        </ListBox>
      </Autocomplete.Filter>
    </Autocomplete.Popover>
  </Autocomplete>
}
