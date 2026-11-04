// Copyright © 2026 Jalapeno Labs

import type { Initiative } from '../../api/routes/initiativeRoutes'
import type { InitiativeEditForm } from './useInitiativeEditor'

// Core
import { useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectAllProjects } from '../../store/projectsSlice'

// User interface
import { UserName } from '../../components/UserName'
import { Button, Card } from '@heroui/react'
import { DayPicker } from '../../components/DayPicker'
import { MultiPicker } from '../../components/MultiPicker'
import { OptionSelect } from '../../components/OptionSelect'

// Misc
import { formatDateTime } from '../../format/dates'
import { INITIATIVE_STATES } from '../../api/routes/initiativeRoutes'
import { useProjectsLoader } from '../../hooks/useServerData'
import { initiativeStateLabelKeys } from './initiativePresentation'

type Props = {
  initiative: Initiative
  // The page's form; these fields are kept by its Save.
  form: InitiativeEditForm
}

// The side panel of an initiative's page: its state, target date, and projects, and who made
// it and when.
export function InitiativeDetailsPanel(props: Props) {
  const { t } = useTranslation([ 'initiatives', 'actionItems', 'common' ])
  useProjectsLoader()
  const projects = useAppSelector(selectAllProjects)
  const initiative = props.initiative
  const form = props.form
  const isReadOnly = Boolean(initiative.deletedAt)
  // Watched by name: the whole-form watch types a class instance as a partial copy.
  const [ state, targetDate, projectIds ] = useWatch({
    control: form.control,
    name: [ 'state', 'targetDate', 'projectIds' ],
  })

  return <Card>
    <Card.Content className='flex flex-col gap-4'>
      <OptionSelect
        label={t('fields.state')}
        isDisabled={isReadOnly}
        options={INITIATIVE_STATES.map((option) => ({ id: option, label: t(initiativeStateLabelKeys[option]) }))}
        value={state}
        onChange={(value) => {
          const chosen = INITIATIVE_STATES.find((option) => option === value)
          if (!chosen) {
            console.debug('InitiativeDetailsPanel ignored an unknown state', { value })
            return
          }
          form.setValue('state', chosen, { shouldDirty: true })
        }}
      />

      <div>
        <DayPicker
          label={t('fields.targetDate')}
          calendarLabel={t('fields.targetDate')}
          isDisabled={isReadOnly}
          value={targetDate}
          onChange={(date) => form.setValue('targetDate', date, { shouldDirty: true })}
        />
        {targetDate && !isReadOnly && <Button
          size='sm'
          variant='ghost'
          className='mt-1'
          onPress={() => form.setValue('targetDate', null, { shouldDirty: true })}
        >
          <span>{t('fields.clearTargetDate')}</span>
        </Button>}
      </div>

      <MultiPicker
        label={t('fields.projects')}
        searchLabel={t('actionItems:fields.searchProjects')}
        emptyLabel={t('actionItems:fields.noProjectsFound')}
        isDisabled={isReadOnly}
        options={projects.map((project) => ({ id: project.id, label: project.name }))}
        value={projectIds}
        onChange={(value) => form.setValue('projectIds', value, { shouldDirty: true })}
      />

      <dl className='grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm'>
        <dt className='opacity-60'>{t('fields.created')}</dt>
        <dd>{formatDateTime(initiative.createdAt)}</dd>
        <dt className='opacity-60'>{t('fields.createdBy')}</dt>
        <dd><UserName userId={initiative.createdBy} /></dd>
        <dt className='opacity-60'>{t('fields.updated')}</dt>
        <dd>{formatDateTime(initiative.updatedAt)}</dd>
      </dl>
    </Card.Content>
  </Card>
}
