// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type { StatusKind } from '../../theme/statusColors'

// User interface
import { StatusIndicator } from '../status/StatusIndicator'

// The standard streaming pattern (docs/ux.md): when work fans out across
// sources (federated Drive search, multi-service sweeps), each source reports
// its own state in the grammar. Fast sources light up `done` instantly while a
// slow one keeps breathing `active`; a failed source turns `error` without
// hiding the results the healthy ones already produced.
export type StreamingSource = {
  id: string
  label: ReactNode
  status: StatusKind
  // A short per-source annotation, e.g. a result count or a one-line error.
  detail?: ReactNode
}

type Props = {
  sources: StreamingSource[]
  className?: string
}

export function StreamingSources(props: Props) {
  if (!props.sources.length) {
    console.debug('StreamingSources rendered with no sources, rendering nothing')
    return null
  }

  return <div className={`flex flex-wrap items-center gap-x-4 gap-y-1 ${props.className ?? ''}`}>
    {props.sources.map((source) => (
      <span key={source.id} className='inline-flex items-center gap-1.5'>
        <StatusIndicator
          size='sm'
          kind={source.status}
          label={source.label}
        />
        {source.detail !== undefined && (
          <span className='text-xs opacity-50'>{
            source.detail
          }</span>
        )}
      </span>
    ))}
  </div>
}
