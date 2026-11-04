// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type { RunStep, StepAction, StepState } from '../../api/routes/runRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Spinner, Tooltip } from '@heroui/react'
import {
  LuBan,
  LuChevronRight,
  LuCircleCheck,
  LuCircleDashed,
  LuCirclePause,
  LuCircleX,
  LuClock,
  LuPlay,
  LuRotateCcw,
  LuSkipForward,
} from 'react-icons/lu'
import { RunDuration } from './RunDuration'
import { StepLog } from './StepLog'

// Misc
import { formatTime } from '../../format/dates'
import { stepActionsByState, stepStateLabelKeys } from './runPresentation'
import { useRunNames } from './useRunNames'
import { useStepActions } from './useStepActions'

type Props = {
  step: RunStep
}

const stateIcons = {
  pending: <LuCircleDashed className='size-4 opacity-50' aria-hidden />,
  queued: <LuClock className='size-4 opacity-70' aria-hidden />,
  running: <Spinner size='sm' />,
  held: <LuCirclePause className='size-4 text-warning' aria-hidden />,
  failed: <LuCircleX className='size-4 text-danger' aria-hidden />,
  skipped: <LuBan className='size-4 opacity-50' aria-hidden />,
  succeeded: <LuCircleCheck className='size-4 text-success' aria-hidden />,
} as const satisfies Record<StepState, ReactNode>

const actionIcons = {
  retry: LuRotateCcw,
  skip: LuSkipForward,
  resume: LuPlay,
} as const satisfies Record<StepAction, typeof LuPlay>

const actionLabelKeys = {
  retry: 'step.actions.retry',
  skip: 'step.actions.skip',
  resume: 'step.actions.resume',
} as const satisfies Record<StepAction, string>

// One step of a run, as GitHub Actions shows one: collapsed to its state, its name, the line
// it wrote (or why it failed), and how long it took; opening it shows its log. A failed step
// starts open, since its log says why. The actions its state allows sit at the row's end.
export function RunStepRow(props: Props) {
  const { t } = useTranslation('runs')
  const act = useStepActions()
  const names = useRunNames()
  const { step } = props
  const [ isOpen, setIsOpen ] = useState(step.state === 'failed')
  const [ pendingAction, setPendingAction ] = useState<StepAction | null>(null)
  const name = names.step(step.step)
  const line = step.summary ?? step.error
  const retriesAt = step.state === 'queued' && step.attempts > 0
    ? step.availableAt
    : null

  async function run(action: StepAction) {
    setPendingAction(action)
    try {
      await act(step, action)
    }
    finally {
      setPendingAction(null)
    }
  }

  return <li className='compact overflow-hidden rounded-lg border border-separator'>
    <div className='level gap-3 px-3 py-2'>
      <button
        type='button'
        className='level-left min-w-0 flex-1 cursor-pointer gap-2 text-left'
        aria-expanded={isOpen}
        aria-label={t(isOpen
          ? 'step.hideLog'
          : 'step.showLog', { step: name })}
        onClick={() => setIsOpen((open) => !open)}
      >
        <LuChevronRight
          className={`size-4 shrink-0 opacity-60 transition-transform ${isOpen ? 'rotate-90' : ''}`}
          aria-hidden
        />
        <span className='grid size-4 shrink-0 place-items-center' title={t(stepStateLabelKeys[step.state])}>{
          stateIcons[step.state]
        }</span>
        <span className='shrink-0 font-medium'>{name}</span>
        {line && <span className={`truncate text-sm ${step.state === 'failed' ? 'text-danger' : 'opacity-70'}`}>{
          line
        }</span>}
      </button>
      <div className='level-right shrink-0 gap-3 text-sm opacity-80'>
        {/* Left out below sm, where the row keeps its name, duration, and actions on one line. */}
        {retriesAt && <span className='hidden sm:inline'>{
          t('run.retryingAt', { time: formatTime(retriesAt) })
        }</span>}
        {step.attempts > 1 && <span className='hidden sm:inline'>{
          t('run.attempts', { count: step.attempts })
        }</span>}
        <RunDuration
          startedAt={step.startedAt}
          finishedAt={step.finishedAt}
          isLive={step.state === 'running'}
        />
      </div>
      {stepActionsByState[step.state].length > 0 && <div className='level-right shrink-0 gap-1'>{
        stepActionsByState[step.state].map((action) => {
          const Icon = actionIcons[action]
          return <Tooltip key={action} delay={300}>
            <Tooltip.Trigger>
              <Button
                isIconOnly
                size='sm'
                variant='ghost'
                aria-label={t(actionLabelKeys[action])}
                isDisabled={pendingAction !== null}
                isPending={pendingAction === action}
                onPress={() => run(action)}
              >
                <Icon className='size-4' aria-hidden />
              </Button>
            </Tooltip.Trigger>
            <Tooltip.Content>
              <span>{t(actionLabelKeys[action])}</span>
            </Tooltip.Content>
          </Tooltip>
        })
      }</div>}
    </div>
    {isOpen && <StepLog runId={step.runId} stepId={step.id} />}
  </li>
}
