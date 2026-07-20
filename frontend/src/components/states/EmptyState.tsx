// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

// The standard empty pattern (docs/ux.md): quiet and reassuring, never an
// error. A dimmed icon, a short headline, an optional line of guidance, and
// (when there is an obvious next step) one primary action.
type Props = {
  icon?: ReactNode
  title: ReactNode
  description?: ReactNode
  action?: ReactNode
  className?: string
}

export function EmptyState(props: Props) {
  return <div className={`flex flex-col items-center justify-center gap-3 py-12 text-center ${props.className ?? ''}`}>
    {props.icon && (
      <div aria-hidden className='text-4xl opacity-30'>{
        props.icon
      }</div>
    )}
    <p className='text-base font-medium opacity-70'>{
      props.title
    }</p>
    {props.description && (
      <p className='max-w-sm text-sm opacity-50'>{
        props.description
      }</p>
    )}
    {props.action && (
      <div className='mt-2'>{
        props.action
      }</div>
    )}
  </div>
}
