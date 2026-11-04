// Copyright © 2026 Jalapeno Labs

import type { KnowledgeFilters } from './knowledgeFilters'

// Core
import { useTranslation } from 'react-i18next'
import { shallowEqual } from 'react-redux'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectProjectNamesById } from '../../store/projectsSlice'
import { selectUserNamesById } from '../../store/usersSlice'

// User interface
import { Button, Tooltip } from '@heroui/react'
import { LuX } from 'react-icons/lu'

// Misc
import { formatCalendarDay } from '../../format/dates'
import { presentKnowledgeSource } from './knowledgePresentation'

type Props = {
  filters: KnowledgeFilters
  onChange: (filters: KnowledgeFilters) => void
}

type FilterTag = {
  id: string
  label: string
  // The filters without this one.
  without: KnowledgeFilters
}

// The filters in force as a row of tags above the results, each removed with one click anywhere
// on it (`~/.claude/docs/ux.md`, Table layout). A long one is cut short, its whole name in its
// tooltip. Nothing renders while no filter is on.
export function KnowledgeFilterTags(props: Props) {
  const { t } = useTranslation('knowledge')
  const projectNames = useAppSelector(selectProjectNamesById, shallowEqual)
  const userNames = useAppSelector(selectUserNamesById, shallowEqual)
  const filters = props.filters

  const tags: FilterTag[] = filters.sources.map((source) => ({
    id: `source:${source}`,
    label: t('filters.tags.source', { source: presentKnowledgeSource(source, t).label }),
    without: { ...filters, sources: filters.sources.filter((kept) => kept !== source) },
  }))
  if (filters.projectId) {
    tags.push({
      id: 'project',
      label: t('filters.tags.project', {
        name: projectNames[filters.projectId] ?? t('result.unknownProject'),
      }),
      without: { ...filters, projectId: null },
    })
  }
  if (filters.personId) {
    tags.push({
      id: 'person',
      label: t('filters.tags.person', {
        name: userNames[filters.personId] ?? t('result.unknownPerson'),
      }),
      without: { ...filters, personId: null },
    })
  }
  if (filters.fromDay) {
    tags.push({
      id: 'since',
      label: t('filters.tags.since', { date: formatCalendarDay(filters.fromDay) }),
      without: { ...filters, fromDay: null },
    })
  }
  if (filters.toDay) {
    tags.push({
      id: 'until',
      label: t('filters.tags.until', { date: formatCalendarDay(filters.toDay) }),
      without: { ...filters, toDay: null },
    })
  }

  if (!tags.length) {
    return null
  }

  return <ul aria-label={t('filters.tags.label')} className='relaxed flex flex-wrap gap-2'>{
    tags.map((tag) => <li key={tag.id}>
      <Tooltip delay={400}>
        <Button
          size='sm'
          variant='secondary'
          className='max-w-64'
          aria-label={t('filters.removeTag', { filter: tag.label })}
          onPress={() => props.onChange(tag.without)}
        >
          <span className='truncate'>{tag.label}</span>
          <LuX className='size-3.5 shrink-0' aria-hidden />
        </Button>
        <Tooltip.Content>{tag.label}</Tooltip.Content>
      </Tooltip>
    </li>)
  }</ul>
}
