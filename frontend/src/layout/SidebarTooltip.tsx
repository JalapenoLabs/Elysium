// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

// User interface
import { Tooltip } from '@heroui/react'

type Props = {
  label: string
  // Only a collapsed sidebar hides its labels, so only then does an entry need its name.
  isEnabled: boolean
  // A React Aria `Link`, which takes the tooltip's hover and focus handlers itself.
  children: ReactNode
}

// Names a sidebar entry to the right of its icon while the sidebar is collapsed.
export function SidebarTooltip(props: Props) {
  return <Tooltip delay={300} isDisabled={!props.isEnabled}>
    {props.children}
    <Tooltip.Content placement='right' offset={12}>
      <span>{
        props.label
      }</span>
    </Tooltip.Content>
  </Tooltip>
}
