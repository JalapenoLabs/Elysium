// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type { StatusKind } from '../../theme/statusColors'

// The grammar dot: a small theme-following disc speaking Elysium's status-color
// language (docs/ux.md), with an optional label beside it. `active` and
// `needsInput` breathe (something is happening / your turn); the terminal
// states hold still. Motion stops under `prefers-reduced-motion` via the
// Tailwind `motion-safe` gate.
type Props = {
  kind: StatusKind
  label?: ReactNode
  size?: 'sm' | 'md'
  className?: string
}

// Tailwind statically extracts class names, so the color classes are spelled
// out per kind rather than interpolated.
const dotClassByKind = {
  idle: 'bg-status-idle',
  active: 'bg-status-active motion-safe:animate-pulse',
  needsInput: 'bg-status-needs-input motion-safe:animate-pulse',
  error: 'bg-status-error',
  done: 'bg-status-done',
} as const satisfies Record<StatusKind, string>

const glowClassByKind = {
  idle: 'shadow-status-idle/40',
  active: 'shadow-status-active/50',
  needsInput: 'shadow-status-needs-input/50',
  error: 'shadow-status-error/50',
  done: 'shadow-status-done/40',
} as const satisfies Record<StatusKind, string>

export function StatusIndicator(props: Props) {
  const {
    size = 'md',
  } = props

  const dotSize = size === 'sm'
    ? 'h-1.5 w-1.5'
    : 'h-2 w-2'

  return <span className={`inline-flex items-center gap-2 ${props.className ?? ''}`}>
    <span
      aria-hidden
      className={
        `rounded-full shadow-[0_0_0.5rem] ${dotSize} ${dotClassByKind[props.kind]} ${glowClassByKind[props.kind]}`
      }
    />
    {props.label !== undefined && (
      <span className={size === 'sm' ? 'text-xs' : 'text-sm'}>{
        props.label
      }</span>
    )}
  </span>
}
