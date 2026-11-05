// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'
import type { KnowledgeFilters } from './knowledgeFilters'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectLabelsInUse } from '../../store/pagesSlice'
import { selectAllProjects } from '../../store/projectsSlice'
import { selectAllUsers } from '../../store/usersSlice'

// User interface
import { Button, Checkbox, CheckboxGroup, Drawer, Form, Label } from '@heroui/react'
import { DayPicker } from '../../components/DayPicker'
import { OptionSelect } from '../../components/OptionSelect'
import { LabelSelect } from './Pages/LabelSelect'

// Utility
import { parseDate } from '@internationalized/date'

// Misc
import { submitOnEnter } from '../../components/submitOnEnter'
import { useProjectsLoader, useUsersLoader } from '../../hooks/useServerData'
import { KNOWLEDGE_SOURCES, presentKnowledgeSource } from './knowledgePresentation'
import { NO_KNOWLEDGE_FILTERS } from './knowledgeFilters'

// The select's choice for no project or no person: the filter is off.
const ANY = 'any'

type Props = {
  isOpen: boolean
  onOpenChange: (isOpen: boolean) => void
  // The filters in force when the drawer opened, which it edits a copy of.
  filters: KnowledgeFilters
  onApply: (filters: KnowledgeFilters) => void
}

// The search's filters, edited in a drawer from the right and applied together, so nothing
// searches while a filter is half chosen (`~/.claude/docs/ux.md`, Table layout and Drawers).
// Its parent remounts it each time it opens, so it always starts from the filters in force.
export function KnowledgeFiltersDrawer(props: Props) {
  const { t } = useTranslation('knowledge')
  const [ draft, setDraft ] = useState(props.filters)
  useProjectsLoader()
  useUsersLoader()
  const projects = useAppSelector(selectAllProjects)
  const users = useAppSelector(selectAllUsers)
  const labelsInUse = useAppSelector(selectLabelsInUse)

  const projectOptions = [
    { id: ANY, label: t('filters.anyProject') },
    ...projects.map((project) => ({ id: project.id, label: project.name })),
  ]
  const personOptions = [{ id: ANY, label: t('filters.anyone') }]
  for (const user of users) {
    if (user.kind === 'person') {
      personOptions.push({ id: user.id, label: user.name })
    }
  }

  function apply(event?: FormEvent) {
    event?.preventDefault()
    props.onApply(draft)
    props.onOpenChange(false)
  }

  // Controlled, so the root trigger wrapper is skipped, as in NotificationDrawer.
  return <Drawer.Backdrop isOpen={props.isOpen} onOpenChange={props.onOpenChange}>
    <Drawer.Content placement='right'>
      <Drawer.Dialog className='gap-4'>
        <Form className='flex h-full flex-col gap-4' onSubmit={apply} onKeyDownCapture={submitOnEnter}>
          <Drawer.Header>
            <Drawer.Heading className='text-lg'>{t('filters.title')}</Drawer.Heading>
          </Drawer.Header>
          <Drawer.Body className='flex-1'>
            <div className='relaxed'>
              <CheckboxGroup
                id='knowledge-filter-sources'
                value={draft.sources}
                onChange={(sources) => setDraft({ ...draft, sources })}
              >
                <Label>{t('filters.sources')}</Label>
                {KNOWLEDGE_SOURCES.map((source) => {
                  const presented = presentKnowledgeSource(source, t)
                  const Icon = presented.icon
                  return <Checkbox key={source} id={`knowledge-filter-source-${source}`} value={source}>
                    <Checkbox.Content>
                      <Checkbox.Control>
                        <Checkbox.Indicator />
                      </Checkbox.Control>
                      <Icon className={`size-4 shrink-0 ${presented.className}`} aria-hidden />
                      <span className='text-sm'>{presented.label}</span>
                    </Checkbox.Content>
                  </Checkbox>
                })}
              </CheckboxGroup>
            </div>
            <div className='compact'>
              <OptionSelect
                id='knowledge-filter-project'
                label={t('filters.project')}
                options={projectOptions}
                value={draft.projectId ?? ANY}
                onChange={(value) => setDraft({
                  ...draft,
                  projectId: value === ANY
                    ? null
                    : value,
                })}
              />
            </div>
            <div className='compact'>
              <OptionSelect
                id='knowledge-filter-person'
                label={t('filters.person')}
                options={personOptions}
                value={draft.personId ?? ANY}
                onChange={(value) => setDraft({
                  ...draft,
                  personId: value === ANY
                    ? null
                    : value,
                })}
              />
            </div>
            <div className='relaxed'>
              <LabelSelect
                id='knowledge-filter-label'
                label={t('filters.pageLabel')}
                labels={labelsInUse}
                value={draft.label}
                onChange={(label) => setDraft({ ...draft, label })}
              />
            </div>
            <div className='compact'>
              <DayPicker
                id='knowledge-filter-since'
                label={t('filters.since')}
                calendarLabel={t('filters.calendar')}
                value={draft.fromDay
                  ? parseDate(draft.fromDay)
                  : null}
                onChange={(day) => setDraft({ ...draft, fromDay: day?.toString() ?? null })}
              />
            </div>
            <div className='compact'>
              <DayPicker
                id='knowledge-filter-until'
                label={t('filters.until')}
                calendarLabel={t('filters.calendar')}
                value={draft.toDay
                  ? parseDate(draft.toDay)
                  : null}
                minValue={draft.fromDay
                  ? parseDate(draft.fromDay)
                  : undefined}
                onChange={(day) => setDraft({ ...draft, toDay: day?.toString() ?? null })}
              />
            </div>
          </Drawer.Body>
          {/* Each action on its own line, the drawer's one action first (ux.md, Drawers). */}
          <Drawer.Footer className='flex-col items-stretch'>
            <Button type='submit' fullWidth>
              <span>{t('filters.apply')}</span>
            </Button>
            <Button fullWidth variant='outline' onPress={() => setDraft(NO_KNOWLEDGE_FILTERS)}>
              <span>{t('filters.clear')}</span>
            </Button>
            <Button fullWidth variant='secondary' onPress={() => props.onOpenChange(false)}>
              <span>{t('filters.cancel')}</span>
            </Button>
          </Drawer.Footer>
        </Form>
      </Drawer.Dialog>
    </Drawer.Content>
  </Drawer.Backdrop>
}
