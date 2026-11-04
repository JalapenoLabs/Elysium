// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { ParseKeys } from 'i18next'
import type { AttributionChangeResponse, TimelineRecord } from '../../api/routes/timelineRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { mutate } from 'swr'

// Redux
import { useAppDispatch, useAppSelector } from '../../store/hooks'
import { selectAllProjects, selectProjectEntities } from '../../store/projectsSlice'
import { timelineRecordUpserted } from '../../store/timelineRecordsSlice'

// User interface
import { Button, Dropdown, Label, Tooltip } from '@heroui/react'
import { LuCheck, LuChevronDown, LuFolderPlus, LuHistory, LuX } from 'react-icons/lu'
import { RecordHistory } from './RecordHistory'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import {
  attributeTimelineRecord,
  recordHistoryKey,
  unattributeTimelineRecord,
} from '../../api/routes/timelineRoutes'
import { formatPercent } from '../../format/numbers'
import { usePendingAction } from '../../hooks/usePendingAction'
import { useProjectsLoader } from '../../hooks/useServerData'
import { toast } from '../../toast/toast'
import { boundedConfidence, isElysiaAttribution, projectsToAdd } from './timelinePresentation'
import { useUndoRecordChange } from './useUndoRecordChange'

type Props = {
  record: TimelineRecord
}

// What a record is about: each project it is on, Elysia's saying how sure she was, which the
// person confirms or takes it off, and Add to a project for one she missed. Every change
// happens at once with Undo in its toast, and History lists every change, Elysia's and theirs.
export function RecordProjects(props: Props) {
  const { t } = useTranslation([ 'timeline', 'common' ])
  const dispatch = useAppDispatch()
  useProjectsLoader()
  const projects = useAppSelector(selectAllProjects)
  const projectsById = useAppSelector(selectProjectEntities)
  const pending = usePendingAction<string>()
  const undo = useUndoRecordChange()
  const [ showsHistory, setShowsHistory ] = useState(false)
  const record = props.record
  const addable = projectsToAdd(record, projects)

  function projectName(projectId: string) {
    return projectsById[projectId]?.name ?? t('attribution.unknownProject')
  }

  // Makes one change, says so, and offers Undo when it changed anything.
  function apply(
    actionKey: string,
    projectId: string,
    request: () => Promise<AttributionChangeResponse>,
    doneKey: ParseKeys<'timeline'>,
  ) {
    return pending.run(actionKey, async () => {
      try {
        const response = await request()
        dispatch(timelineRecordUpserted(response.record))
        void mutate(recordHistoryKey(record.id))
        const message = t(doneKey, { project: projectName(projectId) })
        const entryId = response.entryId
        if (!entryId) {
          toast.success(message)
          return
        }
        toast.success(message, {
          action: {
            label: t('history.undo'),
            onPress: () => void undo(record.id, entryId),
          },
        })
      }
      catch (error) {
        console.debug('RecordProjects failed to change an attribution', { error, recordId: record.id, projectId })
        toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
      }
    })
  }

  function add(key: Key) {
    const projectId = String(key)
    void apply(
      `keep:${projectId}`,
      projectId,
      () => attributeTimelineRecord(record.id, projectId),
      'attribution.added',
    )
  }

  return <div className='flex flex-col gap-2'>
    <div className='flex flex-wrap items-center gap-2 text-xs'>
      {!record.projects.length && <span className='opacity-60'>{t('attribution.none')}</span>}
      {record.projects.map((attribution) => {
        const projectId = attribution.projectId
        const name = projectName(projectId)
        const byElysia = isElysiaAttribution(attribution)
        const confirmLabel = t('attribution.confirm', { project: name })
        const removeLabel = t('attribution.remove', { project: name })
        return <span
          key={projectId}
          className='inline-flex items-center gap-1 rounded-full border border-separator py-0.5 pr-0.5 pl-2.5'
        >
          <span className='font-medium'>{name}</span>
          {byElysia && <span className='opacity-60'>{
            t('attribution.byElysia', { percent: formatPercent(boundedConfidence(attribution.confidence)) })
          }</span>}
          {byElysia && <Tooltip delay={300}>
            <Button
              isIconOnly
              size='sm'
              variant='ghost'
              aria-label={confirmLabel}
              isPending={pending.pendingKey === `keep:${projectId}`}
              isDisabled={pending.isOtherPending(`keep:${projectId}`)}
              onPress={() => void apply(
                `keep:${projectId}`,
                projectId,
                () => attributeTimelineRecord(record.id, projectId),
                'attribution.confirmed',
              )}
            >
              <LuCheck className='size-3.5' aria-hidden />
            </Button>
            <Tooltip.Content>
              <span>{confirmLabel}</span>
            </Tooltip.Content>
          </Tooltip>}
          <Tooltip delay={300}>
            <Button
              isIconOnly
              size='sm'
              variant='ghost'
              aria-label={removeLabel}
              isPending={pending.pendingKey === `remove:${projectId}`}
              isDisabled={pending.isOtherPending(`remove:${projectId}`)}
              onPress={() => void apply(
                `remove:${projectId}`,
                projectId,
                () => unattributeTimelineRecord(record.id, projectId),
                'attribution.removed',
              )}
            >
              <LuX className='size-3.5' aria-hidden />
            </Button>
            <Tooltip.Content>
              <span>{removeLabel}</span>
            </Tooltip.Content>
          </Tooltip>
        </span>
      })}
      {Boolean(addable.length) && <Dropdown>
        <Button size='sm' variant='ghost' isDisabled={pending.isAnyPending}>
          <LuFolderPlus className='size-4' aria-hidden />
          <span>{t('attribution.add')}</span>
          <LuChevronDown className='size-4' aria-hidden />
        </Button>
        <Dropdown.Popover placement='bottom start' className='max-h-80 overflow-y-auto'>
          <Dropdown.Menu aria-label={t('attribution.addLabel')} onAction={add}>{
            addable.map((project) => <Dropdown.Item key={project.id} id={project.id} textValue={project.name}>
              <LuFolderPlus className='size-4' aria-hidden />
              <Label>{project.name}</Label>
            </Dropdown.Item>)
          }</Dropdown.Menu>
        </Dropdown.Popover>
      </Dropdown>}
      <Button size='sm' variant='ghost' onPress={() => setShowsHistory(!showsHistory)}>
        <LuHistory className='size-4' aria-hidden />
        <span>{
          showsHistory
            ? t('history.hide')
            : t('history.open')
        }</span>
      </Button>
    </div>
    {showsHistory && <RecordHistory recordId={record.id} projectName={projectName} />}
  </div>
}
