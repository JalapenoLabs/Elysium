// Copyright © 2026 Jalapeno Labs

// User interface
import { Button } from 'react-aria-components'
import { LuChevronRight } from 'react-icons/lu'

type Props = {
  isExpanded: boolean
}

// What opens and closes a branch of the tree: React Aria's `chevron` slot on the row, which names
// and wires it, turning a quarter as the branch opens.
export function PageTreeChevron(props: Props) {
  return <Button
    slot='chevron'
    className={[
      'grid size-6 shrink-0 cursor-default place-items-center rounded outline-none',
      'hover:bg-foreground/10 focus-visible:ring-2 focus-visible:ring-focus',
    ].join(' ')}
  >
    <LuChevronRight
      aria-hidden
      className={[
        'size-3.5 transition-transform duration-150 motion-reduce:transition-none',
        props.isExpanded
          ? 'rotate-90'
          : 'rotate-0',
      ].join(' ')}
    />
  </Button>
}
