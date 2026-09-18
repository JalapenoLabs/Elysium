// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

type Props = {
  children: ReactNode
}

// What a list shows in place of an empty table: uikit's table has no empty state of its own.
export function EmptyNotice(props: Props) {
  return <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
    props.children
  }</p>
}
