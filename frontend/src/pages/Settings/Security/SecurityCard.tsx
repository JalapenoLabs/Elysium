// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

type Props = {
  title: string
  // Shown beside the title, such as whether two-factor authentication is on.
  status?: ReactNode
  // What the feature holds now, above its actions.
  children?: ReactNode
  // Buttons along the bottom, each opening the modal that does the work.
  actions: ReactNode
}

// One way of signing in, as a card on the General tab: its state, and buttons for changing it.
export function SecurityCard(props: Props) {
  return <section className='relaxed rounded-xl border border-separator p-6'>
    <div className='level compact items-start'>
      <h2 className='text-lg font-semibold'>{props.title}</h2>
      {props.status}
    </div>
    {props.children && <div className='compact'>{
      props.children
    }</div>}
    <div className='level-left flex-wrap'>{
      props.actions
    }</div>
  </section>
}
