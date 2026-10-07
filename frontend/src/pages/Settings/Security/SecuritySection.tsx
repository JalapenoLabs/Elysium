// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

type Props = {
  title: string
  description: string
  // Shown beside the title, such as whether an authenticator is set up.
  status?: ReactNode
  children: ReactNode
}

// One way of signing in, or the profile, as a panel on the Sign-in & security page.
export function SecuritySection(props: Props) {
  return <section className='relaxed rounded-xl border border-separator p-6'>
    <div className='level compact items-start'>
      <div>
        <h2 className='text-lg font-semibold'>{props.title}</h2>
        <p className='mt-1 max-w-2xl text-sm opacity-70'>{props.description}</p>
      </div>
      {props.status}
    </div>
    {props.children}
  </section>
}
