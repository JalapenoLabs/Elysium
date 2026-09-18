// Copyright © 2026 Jalapeno Labs

import type { JiraBoard, JiraBoardId, JiraProject, JiraProjectId } from '../../../api/routes/jiraRoutes'
import type { JiraScopeOption } from './JiraScopePicker'

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Description, Label, Switch } from '@heroui/react'
import { JiraScopePicker } from './JiraScopePicker'

// What Elysium may read on a site: everything the token reaches, including projects and
// boards added later, or the ids picked one by one.
export type JiraScopeSelection = {
  allProjects: boolean
  projectIds: JiraProjectId[]
  allBoards: boolean
  boardIds: JiraBoardId[]
}

type Props = {
  // Everything the token reaches, as Jira reported it.
  projectOptions: JiraProject[]
  boardOptions: JiraBoard[]
  value: JiraScopeSelection
  onChange: (value: JiraScopeSelection) => void
  isDisabled: boolean
}

// Splits picked ids into the ones the token still reaches, which the picker shows as
// tags, and the ones it no longer does, which are kept so that a pick made about
// something else never quietly drops them. The page above says which those are.
function splitByReach(pickedIds: string[], options: JiraScopeOption[]) {
  const offeredIds = new Set(options.map((option) => option.id))
  const offered: string[] = []
  const unreachable: string[] = []

  for (const id of pickedIds) {
    if (offeredIds.has(id)) {
      offered.push(id)
      continue
    }
    unreachable.push(id)
  }

  return { offered, unreachable } as const
}

// The allowlist half of a Jira credential, shared by the second step of adding a site and
// by the edit form.
export function JiraScopeFields(props: Props) {
  const { t } = useTranslation('jira')

  const projectOptions = useMemo(
    () => props.projectOptions.map((project) => ({
      id: project.projectId,
      label: project.name,
      detail: project.projectKey,
    })),
    [ props.projectOptions ],
  )

  const boardOptions = useMemo(
    () => props.boardOptions.map((board) => ({
      id: board.boardId,
      label: board.name,
      detail: board.projectKey,
    })),
    [ props.boardOptions ],
  )

  const pickedProjects = splitByReach(props.value.projectIds, projectOptions)
  const pickedBoards = splitByReach(props.value.boardIds, boardOptions)

  return <div className='flex flex-col gap-4'>
    {/* Projects */}
    <Switch
      isSelected={props.value.allProjects}
      isDisabled={props.isDisabled}
      onChange={(isSelected) => props.onChange({ ...props.value, allProjects: isSelected })}
    >
      <Switch.Control>
        <Switch.Thumb />
      </Switch.Control>
      <Switch.Content>
        <Label>{t('scope.allProjects')}</Label>
        <Description>{t('scope.allProjectsHint')}</Description>
      </Switch.Content>
    </Switch>

    {!props.value.allProjects && (projectOptions.length
      ? <JiraScopePicker
        label={t('scope.projects')}
        description={t('scope.projectsHint')}
        options={projectOptions}
        selectedIds={pickedProjects.offered}
        onChange={(ids) => props.onChange({
          ...props.value,
          projectIds: [ ...pickedProjects.unreachable, ...ids ],
        })}
        isDisabled={props.isDisabled}
      />
      : <p className='text-sm opacity-70'>{t('scope.noProjects')}</p>)}

    {/* Boards */}
    <Switch
      isSelected={props.value.allBoards}
      isDisabled={props.isDisabled}
      onChange={(isSelected) => props.onChange({ ...props.value, allBoards: isSelected })}
    >
      <Switch.Control>
        <Switch.Thumb />
      </Switch.Control>
      <Switch.Content>
        <Label>{t('scope.allBoards')}</Label>
        <Description>{t('scope.allBoardsHint')}</Description>
      </Switch.Content>
    </Switch>

    {!props.value.allBoards && (boardOptions.length
      ? <JiraScopePicker
        label={t('scope.boards')}
        description={t('scope.boardsHint')}
        options={boardOptions}
        selectedIds={pickedBoards.offered}
        onChange={(ids) => props.onChange({
          ...props.value,
          boardIds: [ ...pickedBoards.unreachable, ...ids ],
        })}
        isDisabled={props.isDisabled}
      />
      : <p className='text-sm opacity-70'>{t('scope.noBoards')}</p>)}
  </div>
}
