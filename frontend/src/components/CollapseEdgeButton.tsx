// Copyright © 2026 Jalapeno Labs

// User interface
import { Button, Tooltip } from '@heroui/react'
import { LuChevronLeft } from 'react-icons/lu'

type Props = {
  isCollapsed: boolean
  collapseLabel: string
  expandLabel: string
  onToggle: () => void
  // Where it sits on its panel's edge, and what reveals it: the panel it belongs to is a named
  // Tailwind group, which only a literal class can name.
  className: string
}

// The small round chevron on a side panel's edge, floating slightly over what is beside it
// (`~/.claude/docs/ux.md`, Sidebars). It is hidden until something reveals it, and always shows
// to keyboard focus and on touch screens, which have no hover.
export function CollapseEdgeButton(props: Props) {
  const label = props.isCollapsed
    ? props.expandLabel
    : props.collapseLabel

  return <Tooltip delay={300}>
    <Button
      isIconOnly
      size='sm'
      variant='outline'
      aria-label={label}
      aria-expanded={!props.isCollapsed}
      onPress={props.onToggle}
      className={[
        'absolute -right-3 z-30 size-6 min-w-0 rounded-full bg-surface shadow-sm',
        'opacity-0 transition-opacity duration-200 motion-reduce:transition-none',
        'focus-visible:opacity-100 pointer-coarse:opacity-100',
        props.className,
      ].join(' ')}
    >
      <LuChevronLeft
        aria-hidden
        className={[
          'size-3.5 transition-transform duration-200 motion-reduce:transition-none',
          props.isCollapsed
            ? 'rotate-180'
            : 'rotate-0',
        ].join(' ')}
      />
    </Button>
    <Tooltip.Content placement='right'>
      <span>{
        label
      }</span>
    </Tooltip.Content>
  </Tooltip>
}
