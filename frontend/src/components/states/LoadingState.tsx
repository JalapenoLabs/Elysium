// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

// User interface
import { Spinner } from '@heroui/react'

// The standard loading pattern (docs/ux.md): a centered spinner with an
// optional quiet label. Reserve it for genuinely blocking loads; anything that
// can stream should use the streaming pattern instead so results appear as
// they arrive.
type Props = {
  label?: ReactNode
  className?: string
}

export function LoadingState(props: Props) {
  return <div className={`flex flex-col items-center justify-center gap-3 py-12 ${props.className ?? ''}`}>
    <Spinner size='md' />
    {props.label && (
      <p className='text-sm opacity-50'>{
        props.label
      }</p>
    )}
  </div>
}
