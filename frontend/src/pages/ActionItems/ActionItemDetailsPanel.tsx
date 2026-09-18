// Copyright © 2026 Jalapeno Labs

import type { ActionItem, ActionItemPriority } from '../../api/routes/actionItemRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../../store/hooks'
import { actionItemUpserted } from '../../store/actionItemsSlice'
import { selectLiveInitiatives } from '../../store/initiativesSlice'
import { selectAllProjects } from '../../store/projectsSlice'

// User interface
import { Button, Card, toast } from '@heroui/react'
import { DayPicker } from '../../components/DayPicker'
import { OptionSelect } from '../../components/OptionSelect'
import { MultiPicker } from '../../components/MultiPicker'

// Utility
import { getLocalTimeZone } from '@internationalized/date'

// Misc
import {
  ACTION_ITEM_PRIORITIES,
  addActionItemProject,
  joinInitiative,
  leaveInitiative,
  removeActionItemProject,
  updateActionItem,
} from '../../api/routes/actionItemRoutes'
import { useInitiativesLoader, useProjectsLoader } from '../../hooks/useServerData'
import { toEndOfDayInstant, toLocalDay } from './actionItemDates'
import { describeOwner, priorityLabelKeys } from './actionItemPresentation'
import { getMembershipChanges } from './membershipChanges'

type Props = {
  item: ActionItem
}

// The side panel of an item's page: its fields, each saved as soon as it changes. Snoozing
// and waiting are actions at the top of the page, since they are things done to the item.
export function ActionItemDetailsPanel(props: Props) {
  const { t, i18n } = useTranslation([ 'actionItems', 'common' ])
  const dispatch = useAppDispatch()
  useProjectsLoader()
  useInitiativesLoader()
  const projects = useAppSelector(selectAllProjects)
  const initiatives = useAppSelector(selectLiveInitiatives)
  const item = props.item
  const isReadOnly = Boolean(item.deletedAt)
  const timeZone = getLocalTimeZone()
  const dateTimeFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium', timeStyle: 'short' })
  const owner = describeOwner(item)

  function reportFailure(error: unknown, context: string) {
    console.debug(`ActionItemDetailsPanel failed to ${context}`, { error, itemId: item.id })
    toast.danger(t('common:errors.unexpected'))
  }

  async function saveFields(changes: { priority: ActionItemPriority } | { dueAt: string | null }) {
    try {
      const response = await updateActionItem(item.id, changes)
      dispatch(actionItemUpserted(response.item))
    }
    catch (error) {
      reportFailure(error, 'save a field')
    }
  }

  // One request per project joined or left; each answers the item as it then stands.
  async function saveProjects(projectIds: string[]) {
    const changes = getMembershipChanges(item.projectIds, projectIds)
    try {
      for (const projectId of changes.added) {
        const response = await addActionItemProject(item.id, projectId)
        dispatch(actionItemUpserted(response.item))
      }
      for (const projectId of changes.removed) {
        const response = await removeActionItemProject(item.id, projectId)
        dispatch(actionItemUpserted(response.item))
      }
    }
    catch (error) {
      reportFailure(error, 'change projects')
    }
  }

  async function saveInitiatives(initiativeIds: string[]) {
    const changes = getMembershipChanges(item.initiativeIds, initiativeIds)
    try {
      for (const initiativeId of changes.added) {
        const response = await joinInitiative(item.id, initiativeId)
        dispatch(actionItemUpserted(response.item))
      }
      for (const initiativeId of changes.removed) {
        const response = await leaveInitiative(item.id, initiativeId)
        dispatch(actionItemUpserted(response.item))
      }
    }
    catch (error) {
      reportFailure(error, 'change initiatives')
    }
  }

  return <Card>
    <Card.Content className='flex flex-col gap-4'>
      <OptionSelect
        label={t('fields.priority')}
        isDisabled={isReadOnly}
        options={ACTION_ITEM_PRIORITIES.map((priority) => ({ id: priority, label: t(priorityLabelKeys[priority]) }))}
        value={item.priority}
        onChange={(value) => {
          const priority = ACTION_ITEM_PRIORITIES.find((option) => option === value)
          if (priority && priority !== item.priority) {
            void saveFields({ priority })
          }
        }}
      />

      <div>
        <DayPicker
          label={t('fields.dueDate')}
          calendarLabel={t('fields.dueDate')}
          isDisabled={isReadOnly}
          value={item.dueAt
            ? toLocalDay(item.dueAt, timeZone)
            : null}
          onChange={(date) => {
            if (date) {
              void saveFields({ dueAt: toEndOfDayInstant(date, timeZone) })
            }
          }}
        />
        {item.dueAt && !isReadOnly && <Button
          size='sm'
          variant='ghost'
          className='mt-1'
          onPress={() => void saveFields({ dueAt: null })}
        >
          <span>{t('fields.clearDueDate')}</span>
        </Button>}
      </div>

      <MultiPicker
        label={t('fields.projects')}
        searchLabel={t('fields.searchProjects')}
        emptyLabel={t('fields.noProjectsFound')}
        isDisabled={isReadOnly}
        options={projects.map((project) => ({ id: project.id, label: project.name }))}
        value={item.projectIds}
        onChange={(projectIds) => void saveProjects(projectIds)}
      />

      <MultiPicker
        label={t('fields.initiatives')}
        searchLabel={t('fields.searchInitiatives')}
        emptyLabel={t('fields.noInitiativesFound')}
        isDisabled={isReadOnly}
        options={initiatives.map((initiative) => ({ id: initiative.id, label: initiative.name }))}
        value={item.initiativeIds}
        onChange={(initiativeIds) => void saveInitiatives(initiativeIds)}
      />

      <dl className='grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm'>
        <dt className='opacity-60'>{t('fields.owner')}</dt>
        <dd>{t(owner.key, owner.values)}</dd>
        <dt className='opacity-60'>{t('fields.created')}</dt>
        <dd>{dateTimeFormatter.format(new Date(item.createdAt))}</dd>
        <dt className='opacity-60'>{t('fields.updated')}</dt>
        <dd>{dateTimeFormatter.format(new Date(item.updatedAt))}</dd>
      </dl>
    </Card.Content>
  </Card>
}
