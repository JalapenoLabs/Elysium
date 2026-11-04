// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectStepLog } from '../../store/runsSlice'

// User interface
import { Spinner } from '@heroui/react'

// Misc
import { useStepLogLoader } from '../../hooks/useServerData'

type Props = {
  runId: string
  stepId: string
}

// A step's log, every attempt's, numbered line by line as GitHub Actions shows a step's. It is
// read when the step is opened, and again by the event stream whenever the step writes to it.
export function StepLog(props: Props) {
  const { t } = useTranslation('runs')
  const status = useStepLogLoader(props.runId, props.stepId, true)
  const log = useAppSelector((state) => selectStepLog(state, props.stepId))

  if (log === undefined) {
    return <div className='border-t border-separator px-4 py-3'>{
      status === 'failed'
        ? <p className='text-sm text-danger'>{t('step.logError')}</p>
        : <Spinner size='sm' />
    }</div>
  }
  const lines = log.split('\n')
  if (lines.at(-1) === '') {
    lines.pop()
  }
  if (!lines.length) {
    return <p className='border-t border-separator px-4 py-3 text-sm opacity-70'>{
      t('step.noLog')
    }</p>
  }
  return <ol className='max-h-96 overflow-auto border-t border-separator bg-surface-secondary py-2 font-mono text-xs'>{
    lines.map((line, index) => <li
      // A log grows at its end alone (its oldest lines drop past 64 KiB), so a line's place
      // serves as its key.
      key={index}
      className='grid grid-cols-[3.5rem_1fr] gap-3 px-2'
    >
      <span className='select-none text-right opacity-50'>{index + 1}</span>
      <span className='whitespace-pre-wrap break-words'>{line}</span>
    </li>)
  }</ol>
}
