// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type { IconType } from 'react-icons'

// User interface
import { Link } from 'react-aria-components'

type Props = {
  icon: IconType
  title: string
  href: string
  // Beside the title, such as a count of what waits there.
  badge?: ReactNode
}

// One entry in the settings directory: a tile with its icon and title, all of it one link, so
// it opens anywhere it is clicked and in a new tab too. Borderless at rest, it shows its box
// while the mouse is over it or it has keyboard focus.
//
// React Aria's unstyled `Link`, as the sidebar's links are: HeroUI's `Link` underlines on
// hover, and this one reports hover for a real mouse only, so a tap leaves no stuck box.
export function SettingsDirectoryItem(props: Props) {
  const Icon = props.icon

  return <Link
    href={props.href}
    className={[
      'flex items-center gap-3 rounded-lg border border-transparent px-3 py-2.5 outline-none',
      'transition-colors duration-200 ease-out motion-reduce:transition-none',
      'data-hovered:border-separator data-hovered:bg-surface',
      'data-focus-visible:border-separator data-focus-visible:bg-surface data-focus-visible:ring-2',
      'data-focus-visible:ring-focus',
    ].join(' ')}
  >
    <span className='grid size-8 shrink-0 place-items-center rounded-md border border-separator bg-surface'>
      <Icon className='size-4 text-accent' aria-hidden />
    </span>
    <span className='min-w-0 truncate font-semibold'>{props.title}</span>
    {props.badge}
  </Link>
}
