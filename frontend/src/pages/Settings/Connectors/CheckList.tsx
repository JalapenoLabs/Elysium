// Copyright © 2026 Jalapeno Labs

import type { CheckOutcome, CheckSpec, ConnectorKind } from '../../../api/routes/connectorRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Card, Chip, Spinner } from '@heroui/react'

// Misc
import { checkStateColors, describeOutcome } from './connectorPresentation'

type Props = {
  kind: ConnectorKind
  specs: CheckSpec[]
  // Each check's outcome so far; a check with none has not run.
  outcomes: Array<CheckOutcome & { checkId: string }>
  // When the outcomes were last stored, for a connector's saved results.
  ranAt?: string | null
}

// A kind's checks in the order they run, each with where it stands and why, as Onyx's
// Connector Checks panel shows them.
export function CheckList(props: Props) {
  const { t } = useTranslation('connectors')
  const byId = new Map(props.outcomes.map((outcome) => [ outcome.checkId, outcome ]))
  const done = props.outcomes.filter((outcome) => outcome.state !== 'queued' && outcome.state !== 'running').length
  const labelOf = (checkId: string) => t(`checks.labels.${checkId}`, { defaultValue: checkId })

  return <Card className='relaxed p-4'>
    <div className='compact level'>
      <h3 className='font-semibold'>
        <span>{t('checks.title')}</span>
        <span className='ml-2 text-sm opacity-60'>{
          t('checks.progress', { done, total: props.specs.length })
        }</span>
      </h3>
      {props.ranAt && <span className='text-sm opacity-60'>{
        t('checks.lastRan', { when: new Date(props.ranAt).toLocaleString() })
      }</span>}
    </div>
    {!props.outcomes.length && <p className='compact text-sm opacity-70'>{t('checks.never')}</p>}
    <ul className='flex flex-col gap-2'>{
      props.specs.map((spec) => {
        const outcome = byId.get(spec.id)
        const state = outcome?.state ?? 'queued'
        return <li key={spec.id} className='grid grid-cols-[4.5rem_1fr_auto] items-start gap-3'>
          <span className='pt-0.5'>{
            state === 'running'
              ? <Spinner size='sm' />
              : <Chip size='sm' variant='soft' color={checkStateColors[state]}>{t(`checks.states.${state}`)}</Chip>
          }</span>
          <div className='min-w-0'>
            <p className='text-sm font-medium'>{labelOf(spec.id)}</p>
            {outcome && <p className='text-sm opacity-70 break-words'>{
              describeOutcome(t, outcome, labelOf, spec.id)
            }</p>}
          </div>
          {spec.required && <span className='text-xs opacity-60'>{t('checks.required')}</span>}
        </li>
      })
    }</ul>
  </Card>
}
