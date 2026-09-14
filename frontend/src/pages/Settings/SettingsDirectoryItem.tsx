// Copyright © 2026 Jalapeno Labs

import type { IconType } from 'react-icons'

// User interface
import { Link } from '@heroui/react'

type Props = {
  icon: IconType
  title: string
  description: string
  href: string
}

// One entry in the settings directory: an icon tile beside a linked title and a
// short description of what lives behind it.
export function SettingsDirectoryItem(props: Props) {
  const Icon = props.icon

  return <div className='flex gap-3'>
    <div className='grid size-8 shrink-0 place-items-center rounded-md border border-separator bg-surface'>
      <Icon className='size-4 text-accent' aria-hidden />
    </div>
    <div className='min-w-0'>
      <Link
        href={props.href}
        className='font-semibold text-link no-underline hover:underline'
      >
        {props.title}
      </Link>
      <p className='mt-0.5 text-sm opacity-70'>{
        props.description
      }</p>
    </div>
  </div>
}
