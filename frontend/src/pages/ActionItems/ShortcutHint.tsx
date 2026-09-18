// Copyright © 2026 Jalapeno Labs

// User interface
import { Kbd } from '@heroui/react'

type Props = {
  shortcut: string
}

// The key that triggers a quick action, shown beside its button's label. Hidden from
// screen readers, which announce the button by its label.
export function ShortcutHint(props: Props) {
  return <Kbd className='ml-1 hidden sm:inline-flex' aria-hidden>
    <Kbd.Content>{props.shortcut.toUpperCase()}</Kbd.Content>
  </Kbd>
}
