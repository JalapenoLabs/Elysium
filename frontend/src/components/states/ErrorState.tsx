// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button } from '@heroui/react'
import { StatusIndicator } from '../status/StatusIndicator'

// The standard error pattern (docs/ux.md): the red grammar dot names the
// state, the copy stays calm and actionable, and a retry is offered whenever
// the caller can retry. Never a wall of stack trace.
type Props = {
  title: ReactNode
  description?: ReactNode
  onRetry?: () => void
  className?: string
}

export function ErrorState(props: Props) {
  const { t } = useTranslation()

  return <div className={`flex flex-col items-center justify-center gap-3 py-12 text-center ${props.className ?? ''}`}>
    <StatusIndicator
      kind='error'
      label={<span className='font-medium text-status-error'>{props.title}</span>}
    />
    {props.description && (
      <p className='max-w-sm text-sm opacity-60'>{
        props.description
      }</p>
    )}
    {props.onRetry && (
      <Button
        size='sm'
        variant='bordered'
        className='mt-2'
        onPress={props.onRetry}
      >
        <span>{
          t('actions.retry')
        }</span>
      </Button>
    )}
  </div>
}
