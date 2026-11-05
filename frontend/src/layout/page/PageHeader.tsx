// Copyright © 2026 Jalapeno Labs

import type { ReactNode, RefCallback } from 'react'
import type { PageAction } from './pageActions'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { buttonVariants, Link, Tooltip } from '@heroui/react'
import { LuArrowLeft } from 'react-icons/lu'
import { HelpPopover } from '../../components/HelpPopover'
import { InlineEditableText } from '../../components/InlineEditableText'
import { PageActions } from './PageActions'
import { PageBreadcrumbs } from './PageBreadcrumbs'

export type PageCrumb = {
  label: string
  href: string
}

// Makes the title its own editor, for a page about one record that is renamed in place. The
// title is a field of the page's form: `title` is what the form holds, and the page's Save
// keeps a new one.
export type PageTitleRename = {
  // Describes the editor, for assistive technology, such as "Rename project".
  label: string
  maxLength: number
  onChange: (title: string) => void
}

export type PageHeaderProps = {
  // The page's name: the large title, and the breadcrumbs' last crumb.
  title: string
  rename?: PageTitleRename
  // Leads the title, such as a connector kind's logo.
  icon?: ReactNode
  // Explains the page in a popover behind a `?` beside the title. Only when it truly needs
  // explaining: the standard page has no subtitle.
  help?: ReactNode
  // Follows the title, such as the record's state chip.
  badges?: ReactNode
  // The pages above this one, outermost first. The last is where the back button goes.
  // Omit on a top-level page, which has neither breadcrumbs nor a back button.
  parents?: PageCrumb[]
  // Leads the titlebar's right side, such as a list's own search field.
  tools?: ReactNode
  // Receives the element after `tools` that the page's list table renders its search,
  // filters, Columns, and export into (`pageTools.ts`). `Page` passes it while a list claims
  // the slot.
  toolsSlotRef?: RefCallback<HTMLDivElement>
  // The one action that stands on its own, such as Apply or New project.
  primaryAction?: ReactNode
  // The page's Save. `Page` fills it from the form that registered one.
  save?: ReactNode
  // Everything else the page can do. A lone harmless action is a button; more become the
  // Actions menu, always on the far right.
  actions?: PageAction[]
  // Spacing around the header. Defaults to the gap a page's content leaves below it.
  className?: string
}

// The standard top of a page (`~/.claude/docs/ux.md`, Standard page layout): breadcrumbs led
// by a back button, then the titlebar, the large title on the left and its tools and actions
// on the right, centered on one line. On phones the titlebar stacks, its buttons full width.
// The right side is drawn only while one of its props asks for it, so whatever a page gives
// it after its first render (a Save registered once the data loads) shows the moment it is
// given. See docs/frontend.md, Page header.
export function PageHeader(props: PageHeaderProps) {
  const { t } = useTranslation('common')
  const parent = props.parents?.at(-1)
  const hasEnd = Boolean(
    props.tools || props.toolsSlotRef || props.primaryAction || props.save || props.actions?.length,
  )

  return <header className={props.className ?? 'relaxed'}>
    {parent && <nav aria-label={t('page.breadcrumbs')} className='compact flex min-w-0 items-center gap-1'>
      <Tooltip delay={300}>
        {/* A link, not a button, so it can be opened in a new tab like any other. */}
        <Link
          href={parent.href}
          aria-label={t('page.back', { page: parent.label })}
          className={`shrink-0 ${buttonVariants({ isIconOnly: true, size: 'sm', variant: 'ghost' })}`}
        >
          <LuArrowLeft className='size-4' aria-hidden />
        </Link>
        <Tooltip.Content>
          <span>{t('page.back', { page: parent.label })}</span>
        </Tooltip.Content>
      </Tooltip>
      <PageBreadcrumbs parents={props.parents ?? []} title={props.title} />
    </nav>}

    <div className='flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between sm:gap-4'>
      {/* Wraps, so a chip after a long title moves below it rather than squeezing it. */}
      <div className='flex min-w-0 flex-1 flex-wrap items-center gap-x-3 gap-y-2'>
        {props.icon}
        {props.rename
          // Takes the free width: the editor's negative margin leaves its natural width a
          // few pixels short of its text, which would wrap a title that fits.
          ? <h1 className='min-w-0 flex-1'>
            <InlineEditableText
              // A page has one title, so one id names it on every page.
              id='page-title'
              value={props.title}
              label={props.rename.label}
              isRequired
              maxLength={props.rename.maxLength}
              className='text-3xl font-bold'
              onChange={props.rename.onChange}
            />
          </h1>
          : <h1 className='min-w-0 text-3xl font-bold break-words'>{
            props.title
          }</h1>}
        {props.help && <HelpPopover>{props.help}</HelpPopover>}
        {props.badges}
      </div>
      {hasEnd && <div className='page-actions'>
        {props.tools}
        {/* Holds no box of its own, so what a table renders into it lines up with the rest. */}
        {props.toolsSlotRef && <div ref={props.toolsSlotRef} className='page-tools-slot contents' />}
        {props.primaryAction}
        <PageActions actions={props.actions ?? []} save={props.save} />
      </div>}
    </div>
  </header>
}
