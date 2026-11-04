// Copyright © 2026 Jalapeno Labs

import type { ActionItem } from '../../api/routes/actionItemRoutes'
import type { ActionItemEditForm } from './useActionItemEditor'

// Core
import { useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectLiveInitiatives } from '../../store/initiativesSlice'
import { selectAllProjects } from '../../store/projectsSlice'

// User interface
import { UserName } from '../../components/UserName'
import { Button, Card } from '@heroui/react'
import { DayPicker } from '../../components/DayPicker'
import { OptionSelect } from '../../components/OptionSelect'
import { MultiPicker } from '../../components/MultiPicker'

// Misc
import { formatDateTime } from '../../format/dates'
import { ACTION_ITEM_PRIORITIES } from '../../api/routes/actionItemRoutes'
import { useInitiativesLoader, useProjectsLoader } from '../../hooks/useServerData'
import { describeOwner, priorityLabelKeys } from './actionItemPresentation'

type Props = {
  item: ActionItem
  // The page's form; these fields are kept by its Save.
  form: ActionItemEditForm
}

// The side panel of an item's page: its fields, and who made it and when. Snoozing and
// waiting are actions at the top of the page, since they are things done to the item.
export function ActionItemDetailsPanel(props: Props) {
  const { t } = useTranslation([ 'actionItems', 'common' ])
  useProjectsLoader()
  useInitiativesLoader()
  const projects = useAppSelector(selectAllProjects)
  const initiatives = useAppSelector(selectLiveInitiatives)
  const item = props.item
  const form = props.form
  const isReadOnly = Boolean(item.deletedAt)
  const owner = describeOwner(item)
  // Watched by name: the whole-form watch types a class instance as a partial copy.
  const [ priority, dueDate, projectIds, initiativeIds ] = useWatch({
    control: form.control,
    name: [ 'priority', 'dueDate', 'projectIds', 'initiativeIds' ],
  })

  return <Card>
    <Card.Content className='flex flex-col gap-4'>
      <OptionSelect
        label={t('fields.priority')}
        isDisabled={isReadOnly}
        options={ACTION_ITEM_PRIORITIES.map((option) => ({ id: option, label: t(priorityLabelKeys[option]) }))}
        value={priority}
        onChange={(value) => {
          const chosen = ACTION_ITEM_PRIORITIES.find((option) => option === value)
          if (!chosen) {
            console.debug('ActionItemDetailsPanel ignored an unknown priority', { value })
            return
          }
          form.setValue('priority', chosen, { shouldDirty: true })
        }}
      />

      <div>
        <DayPicker
          label={t('fields.dueDate')}
          calendarLabel={t('fields.dueDate')}
          isDisabled={isReadOnly}
          value={dueDate}
          onChange={(date) => form.setValue('dueDate', date, { shouldDirty: true })}
        />
        {dueDate && !isReadOnly && <Button
          size='sm'
          variant='ghost'
          className='mt-1'
          onPress={() => form.setValue('dueDate', null, { shouldDirty: true })}
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
        value={projectIds}
        onChange={(value) => form.setValue('projectIds', value, { shouldDirty: true })}
      />

      <MultiPicker
        label={t('fields.initiatives')}
        searchLabel={t('fields.searchInitiatives')}
        emptyLabel={t('fields.noInitiativesFound')}
        isDisabled={isReadOnly}
        options={initiatives.map((initiative) => ({ id: initiative.id, label: initiative.name }))}
        value={initiativeIds}
        onChange={(value) => form.setValue('initiativeIds', value, { shouldDirty: true })}
      />

      <dl className='grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm'>
        <dt className='opacity-60'>{t('fields.owner')}</dt>
        <dd>{t(owner.key, owner.values)}</dd>
        <dt className='opacity-60'>{t('fields.created')}</dt>
        <dd>{formatDateTime(item.createdAt)}</dd>
        <dt className='opacity-60'>{t('fields.createdBy')}</dt>
        <dd><UserName userId={item.createdBy} /></dd>
        <dt className='opacity-60'>{t('fields.updated')}</dt>
        <dd>{formatDateTime(item.updatedAt)}</dd>
      </dl>
    </Card.Content>
  </Card>
}
