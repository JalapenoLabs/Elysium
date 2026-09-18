// Copyright © 2026 Jalapeno Labs

import type { JiraBoard, JiraProject } from '../../../api/routes/jiraRoutes'
import type { JiraScopeOption } from './JiraScopePicker'
import type { JiraScopeSelection } from './jiraPresentation'

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Description, Label, Switch } from '@heroui/react'
import { JiraScopePicker } from './JiraScopePicker'

type Props = {
  // Everything the token reaches, as Jira reported it.
  projectOptions: JiraProject[]
  boardOptions: JiraBoard[]
  // True when Jira held more than Elysium reads, so the list below is not the whole of it.
  projectsTruncated: boolean
  boardsTruncated: boolean
  value: JiraScopeSelection
  onChange: (value: JiraScopeSelection) => void
  // Each list locks on its own: one may be missing a stored selection while the other is
  // whole. The form above decides; this only renders it.
  isProjectsDisabled: boolean
  isBoardsDisabled: boolean
}

// The picked ids the picker can show as tags. A stored id the token no longer reaches is
// not among the options, so it has no tag; the page above names those in a warning, and
// the form decides what becomes of them when it saves.
function pickedFromOptions(pickedIds: string[], options: JiraScopeOption[]) {
  const offeredIds = new Set(options.map((option) => option.id))
  return pickedIds.filter((id) => offeredIds.has(id))
}

// The allowlist half of a Jira credential, shared by the second step of adding a site and
// by the edit form.
export function JiraScopeFields(props: Props) {
  const { t } = useTranslation('jira')

  const projectOptions = useMemo(
    () => props.projectOptions.map((project) => ({
      id: project.id,
      label: project.name,
      detail: project.key,
    })),
    [ props.projectOptions ],
  )

  // A board's id is a number over the API and a key in the picker, so this is the one
  // place the two spellings meet.
  const boardOptions = useMemo(
    () => props.boardOptions.map((board) => ({
      id: String(board.id),
      label: board.name,
      detail: board.projectKey,
    })),
    [ props.boardOptions ],
  )

  const pickedProjects = pickedFromOptions(props.value.projectIds, projectOptions)
  const pickedBoards = pickedFromOptions(props.value.boardIds.map(String), boardOptions)

  return <div className='flex flex-col gap-4'>
    {/* Projects */}
    <Switch
      isSelected={props.value.allProjects}
      isDisabled={props.isProjectsDisabled}
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
        selectedIds={pickedProjects}
        onChange={(ids) => props.onChange({ ...props.value, projectIds: ids })}
        isDisabled={props.isProjectsDisabled}
      />
      : <p className='text-sm opacity-70'>{t('scope.noProjects')}</p>)}

    {!props.value.allProjects && props.projectsTruncated && <p className='text-sm opacity-70'>{
      t('scope.truncated')
    }</p>}

    {/* Boards */}
    <Switch
      isSelected={props.value.allBoards}
      isDisabled={props.isBoardsDisabled}
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
        selectedIds={pickedBoards}
        onChange={(ids) => props.onChange({ ...props.value, boardIds: ids.map(Number) })}
        isDisabled={props.isBoardsDisabled}
      />
      : <p className='text-sm opacity-70'>{t('scope.noBoards')}</p>)}

    {!props.value.allBoards && props.boardsTruncated && <p className='text-sm opacity-70'>{
      t('scope.truncated')
    }</p>}
  </div>
}
