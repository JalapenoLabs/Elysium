// Copyright © 2026 Jalapeno Labs

import type { ActionItem, ManagerPlan } from '../../api/routes/actionItemRoutes'

// Core
import { useTranslation } from 'react-i18next'
import { shallowEqual } from 'react-redux'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectIsCompanyMember } from '../../store/authSlice'
import { selectProjectNamesById } from '../../store/projectsSlice'

// User interface
import { Button, Chip, Link, Spinner, Tooltip } from '@heroui/react'
import { LuUndo2, LuWorkflow } from 'react-icons/lu'
import { HelpPopover } from '../../components/HelpPopover'
import { UserName } from '../../components/UserName'

// Misc
import { formatDateTime } from '../../format/dates'
import { useManagerPlan, useProjectsLoader } from '../../hooks/useServerData'
import { usePendingAction } from '../../hooks/usePendingAction'
import { getProjectManagerUrl, getWorkBatchViewUrl } from '../../urls'
import { batchStateChipColors, batchStateLabelKeys } from '../Projects/projectManagerPresentation'
import { managerStageChipColors, managerStageLabelKeys } from './actionItemPresentation'
import { useActionItemActions } from './useActionItemActions'

type Props = {
  item: ActionItem
}

// Who an item is assigned to, and where its plan stands: assigning wakes its lead project's
// Project Manager to plan it, and what it plans waits for the person's approval. An item no
// project leads cannot be assigned, and says so. The company's members alone see it, as they
// alone see Project Managers. A finished item shows none: finishing it ends its assignment.
export function ActionItemManagerAssignment(props: Props) {
  const { t } = useTranslation([ 'actionItems', 'projects', 'common' ])
  useProjectsLoader()
  const isMember = useAppSelector(selectIsCompanyMember)
  const projectNames = useAppSelector(selectProjectNamesById, shallowEqual)
  const actions = useActionItemActions()
  const pending = usePendingAction<'assign' | 'unassign'>()
  const item = props.item
  const isAssigned = Boolean(item.managerAssignment)
  const { plan, status } = useManagerPlan(item.id, isMember && isAssigned)
  const isFinished = item.state === 'resolved' || item.state === 'dismissed'

  if (!isMember || item.deletedAt || isFinished) {
    return null
  }

  const leadName = item.leadProjectId
    ? projectNames[item.leadProjectId] ?? t('history.values.deletedProject')
    : null

  // The button wraps below the heading on a phone rather than breaking the heading's line.
  return <section className='relaxed'>
    <div className='compact flex flex-wrap items-center justify-between gap-2'>
      <div className='level-left gap-1'>
        <h2 className='text-lg font-semibold whitespace-nowrap'>{t('manager.heading')}</h2>
        <HelpPopover label={t('manager.helpLabel')}>
          <span>{t('manager.help')}</span>
        </HelpPopover>
      </div>
      {isAssigned
        ? <Button
          size='sm'
          variant='ghost'
          isPending={pending.pendingKey === 'unassign'}
          onPress={() => void pending.run('unassign', () => actions.unassignFromManager(item))}
        >
          <LuUndo2 className='size-4' aria-hidden />
          <span>{t('manager.unassign')}</span>
        </Button>
        : <Tooltip isDisabled={Boolean(item.leadProjectId)}>
          <div>
            <Button
              size='sm'
              variant='secondary'
              isDisabled={!item.leadProjectId}
              isPending={pending.pendingKey === 'assign'}
              onPress={() => void pending.run('assign', () => actions.assignToManager(item))}
            >
              <LuWorkflow className='size-4' aria-hidden />
              <span>{t('manager.assign')}</span>
            </Button>
          </div>
          <Tooltip.Content>{t('manager.noLeadTooltip')}</Tooltip.Content>
        </Tooltip>}
    </div>

    {!isAssigned && <p className='text-sm opacity-70'>{
      leadName
        ? t('manager.notAssigned')
        : t('manager.noLead')
    }</p>}
    {isAssigned && status === 'loading' && <div className='grid place-items-center py-4'>
      <Spinner size='sm' />
    </div>}
    {isAssigned && status === 'failed' && <p className='text-sm text-danger'>{t('manager.loadError')}</p>}
    {plan && <AssignedPlan plan={plan} projectName={leadName ?? t('history.values.deletedProject')} />}
  </section>
}

type AssignedPlanProps = {
  plan: ManagerPlan
  projectName: string
}

// An assignment's details, its stage, and the batches planned for the item, each opening its page.
function AssignedPlan(props: AssignedPlanProps) {
  const { t } = useTranslation([ 'actionItems', 'projects' ])
  const plan = props.plan
  const managerUrl = getProjectManagerUrl(plan.projectId)

  return <div className='flex flex-col gap-3 text-sm'>
    <Link href={managerUrl} className='text-link'>{
      t('manager.assignedTo', { project: props.projectName })
    }</Link>
    <dl className='grid grid-cols-[auto_1fr] gap-x-4 gap-y-1'>
      <dt className='opacity-60'>{t('manager.assignedBy')}</dt>
      <dd><UserName userId={plan.assignedBy} /></dd>
      <dt className='opacity-60'>{t('manager.assignedOn')}</dt>
      <dd>{formatDateTime(plan.assignedAt)}</dd>
      <dt className='opacity-60'>{t('manager.stage')}</dt>
      <dd>
        <Chip size='sm' variant='soft' color={managerStageChipColors[plan.stage]}>{
          t(managerStageLabelKeys[plan.stage])
        }</Chip>
      </dd>
    </dl>
    {plan.stage === 'failed' && <p className='opacity-70'>{t('manager.failedHint')}</p>}
    {plan.stage === 'planned' && !plan.batches.length && <p className='opacity-70'>{t('manager.noBatches')}</p>}
    {plan.batches.length > 0 && <div>
      <h3 className='compact text-xs font-semibold uppercase opacity-60'>{t('manager.batches')}</h3>
      <ul className='flex flex-col gap-2'>{
        plan.batches.map((batch) => <li key={batch.id} className='level gap-3'>
          {/* The title clamps inside the link, which HeroUI's styles keep from truncating itself. */}
          <Link href={getWorkBatchViewUrl(batch.id)} className='min-w-0 text-link'>
            <span className='line-clamp-2'>{batch.title}</span>
          </Link>
          <Chip size='sm' variant='soft' className='shrink-0' color={batchStateChipColors[batch.state]}>{
            t(batchStateLabelKeys[batch.state], { ns: 'projects' })
          }</Chip>
        </li>)
      }</ul>
    </div>}
  </div>
}
