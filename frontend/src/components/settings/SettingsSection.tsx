// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

// The building blocks of the settings pages, modeled on Stripe's settings: flat
// sections separated only by hairlines (never per-field), tight aligned
// label/value rows, and small, low-radius controls. No cards, no heavy
// rounding. Kept here so every settings surface reads as one system.

type SettingsSectionProps = {
  title: string
  description?: string
  // A control aligned to the right of the header (e.g. Edit, Add).
  action?: ReactNode
  children: ReactNode
}

// One settings section: a header (title + optional subtext on the left, an
// action on the right) over its content, closed by a hairline that separates it
// from the next section.
export function SettingsSection(props: SettingsSectionProps) {
  return <section className='border-b border-divider py-7 first:pt-0 last:border-b-0'>
    <div className='flex items-start justify-between gap-4'>
      <div>
        <h2 className='text-base font-semibold'>{ props.title }</h2>
        { props.description
          ? <p className='mt-0.5 text-sm text-default-500'>{ props.description }</p>
          : null }
      </div>
      { props.action
        ? <div className='shrink-0'>{ props.action }</div>
        : null }
    </div>
    <div className='mt-5'>{ props.children }</div>
  </section>
}

// A read-view row: a fixed-width muted label aligned left, its value beside it.
// No divider; rows within a section are separated by spacing alone.
export function DetailRow(props: { label: string, children: ReactNode }) {
  return <div className='flex gap-4 py-2'>
    <div className='w-44 shrink-0 text-sm text-default-500'>{ props.label }</div>
    <div className='min-w-0 flex-1 text-sm'>{ props.children }</div>
  </div>
}
