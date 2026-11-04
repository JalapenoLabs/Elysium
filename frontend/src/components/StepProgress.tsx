// Copyright © 2026 Jalapeno Labs

import type { NavigateOptions } from 'react-router'

// User interface
import { Link } from '@heroui/react'
import { LuCheck } from 'react-icons/lu'

export type ProgressStep = {
  id: string
  label: string
  // Where going back to this step leads. Only steps before the current one are links.
  href: string
}

type Props = {
  steps: ProgressStep[]
  current: string
  label: string
  // How the links navigate, such as without the leave prompt, since going back a step stays on
  // the page.
  routerOptions?: NavigateOptions
}

// The steps of a multi-step form along its top, the current one marked, each finished one a
// link back to it (`~/.claude/docs/ux.md`, Form layout). Steps ahead are not links: a form
// moves forward through its own Next.
export function StepProgress(props: Props) {
  const currentIndex = props.steps.findIndex((step) => step.id === props.current)

  return <nav aria-label={props.label} className='relaxed'>
    <ol className='flex flex-wrap items-center gap-x-2 gap-y-3'>{
      props.steps.map((step, index) => {
        const isDone = index < currentIndex
        const isCurrent = index === currentIndex
        const marker = <span className={[
          'grid size-7 shrink-0 place-items-center rounded-full border text-sm font-semibold',
          'transition-colors duration-200 motion-reduce:transition-none',
          isCurrent
            ? 'border-accent bg-accent text-accent-foreground'
            : 'border-separator',
          isDone
            ? 'bg-surface-secondary'
            : '',
        ].join(' ')}>{
            isDone
              ? <LuCheck className='size-4' aria-hidden />
              : index + 1
          }</span>

        return <li key={step.id} className='flex items-center gap-2'>
          {isDone
            ? <Link
              href={step.href}
              routerOptions={props.routerOptions}
              className='flex items-center gap-2 text-sm no-underline hover:opacity-80'
            >
              {marker}
              <span>{step.label}</span>
            </Link>
            : <span
              className={isCurrent
                ? 'flex items-center gap-2 text-sm font-semibold'
                : 'flex items-center gap-2 text-sm opacity-60'}
              aria-current={isCurrent
                ? 'step'
                : undefined}
            >
              {marker}
              <span>{step.label}</span>
            </span>}
          {index < props.steps.length - 1 && <span className='hidden h-px w-8 bg-separator sm:block' aria-hidden />}
        </li>
      })
    }</ol>
  </nav>
}
