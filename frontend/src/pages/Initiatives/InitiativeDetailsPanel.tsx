// Copyright © 2026 Jalapeno Labs

import type { Initiative, InitiativeState } from '../../api/routes/initiativeRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../../store/hooks'
import { initiativeUpserted } from '../../store/initiativesSlice'
import { selectAllProjects } from '../../store/projectsSlice'

// User interface
import { Button, Card, toast } from '@heroui/react'
import { DayPicker } from '../../components/DayPicker'
import { MultiPicker } from '../../components/MultiPicker'
import { OptionSelect } from '../../components/OptionSelect'

// Utility
import { getLocalTimeZone } from '@internationalized/date'

// Misc
import {
  addInitiativeProject,
  INITIATIVE_STATES,
  removeInitiativeProject,
  updateInitiative,
} from '../../api/routes/initiativeRoutes'
import { useProjectsLoader } from '../../hooks/useServerData'
import { toEndOfDayInstant, toLocalDay } from '../ActionItems/actionItemDates'
import { getMembershipChanges } from '../ActionItems/membershipChanges'
import { initiativeStateLabelKeys } from './initiativePresentation'

type Props = {
  initiative: Initiative
}

// The side panel of an initiative's page: its state, target date, and projects, each saved
// as soon as it changes.
export function InitiativeDetailsPanel(props: Props) {
  const { t, i18n } = useTranslation([ 'initiatives', 'actionItems', 'common' ])
  const dispatch = useAppDispatch()
  useProjectsLoader()
  const projects = useAppSelector(selectAllProjects)
  const initiative = props.initiative
  const isReadOnly = Boolean(initiative.deletedAt)
  const timeZone = getLocalTimeZone()
  const dateTimeFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium', timeStyle: 'short' })

  function reportFailure(error: unknown, context: string) {
    console.debug(`InitiativeDetailsPanel failed to ${context}`, { error, initiativeId: initiative.id })
    toast.danger(t('common:errors.unexpected'))
  }

  async function saveFields(changes: { state: InitiativeState } | { targetAt: string | null }) {
    try {
      const response = await updateInitiative(initiative.id, changes)
      dispatch(initiativeUpserted(response.initiative))
    }
    catch (error) {
      reportFailure(error, 'save a field')
    }
  }

  // One request per project joined or left; each answers the initiative as it then stands.
  async function saveProjects(projectIds: string[]) {
    const changes = getMembershipChanges(initiative.projectIds, projectIds)
    try {
      for (const projectId of changes.added) {
        const response = await addInitiativeProject(initiative.id, projectId)
        dispatch(initiativeUpserted(response.initiative))
      }
      for (const projectId of changes.removed) {
        const response = await removeInitiativeProject(initiative.id, projectId)
        dispatch(initiativeUpserted(response.initiative))
      }
    }
    catch (error) {
      reportFailure(error, 'change projects')
    }
  }

  return <Card>
    <Card.Content className='flex flex-col gap-4'>
      <OptionSelect
        label={t('fields.state')}
        isDisabled={isReadOnly}
        options={INITIATIVE_STATES.map((state) => ({ id: state, label: t(initiativeStateLabelKeys[state]) }))}
        value={initiative.state}
        onChange={(value) => {
          const state = INITIATIVE_STATES.find((option) => option === value)
          if (state && state !== initiative.state) {
            void saveFields({ state })
          }
        }}
      />

      <div>
        <DayPicker
          label={t('fields.targetDate')}
          calendarLabel={t('fields.targetDate')}
          isDisabled={isReadOnly}
          value={initiative.targetAt
            ? toLocalDay(initiative.targetAt, timeZone)
            : null}
          onChange={(date) => {
            if (date) {
              void saveFields({ targetAt: toEndOfDayInstant(date, timeZone) })
            }
          }}
        />
        {initiative.targetAt && !isReadOnly && <Button
          size='sm'
          variant='ghost'
          className='mt-1'
          onPress={() => void saveFields({ targetAt: null })}
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
        value={initiative.projectIds}
        onChange={(projectIds) => void saveProjects(projectIds)}
      />

      <dl className='grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm'>
        <dt className='opacity-60'>{t('fields.created')}</dt>
        <dd>{dateTimeFormatter.format(new Date(initiative.createdAt))}</dd>
        <dt className='opacity-60'>{t('fields.updated')}</dt>
        <dd>{dateTimeFormatter.format(new Date(initiative.updatedAt))}</dd>
      </dl>
    </Card.Content>
  </Card>
}
