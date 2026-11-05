// Copyright © 2026 Jalapeno Labs

import type { PageCrumb } from './PageHeader'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Breadcrumbs, Button, Dropdown, Label, Tooltip } from '@heroui/react'
import { LuChevronRight, LuCornerLeftUp, LuEllipsis } from 'react-icons/lu'

type Props = {
  parents: PageCrumb[]
  title: string
}

// Most pages sit a level or two down, and a knowledge base page sits as deep as its tree goes.
const MOST_PARENTS_SHOWN = 3

// A path too long for one line keeps where it starts and the parent beside the page, and the
// rest fold into a menu. A menu rather than wrapping, so every page's title stays level.
export function foldPageCrumbs(parents: PageCrumb[]) {
  if (parents.length <= MOST_PARENTS_SHOWN) {
    return { leading: parents, folded: [], trailing: []} as const
  }
  return {
    leading: parents.slice(0, 1),
    folded: parents.slice(1, -1),
    trailing: parents.slice(-1),
  } as const
}

function renderCrumb(crumb: PageCrumb) {
  return <Breadcrumbs.Item key={crumb.href} href={crumb.href}>{
    crumb.label
  }</Breadcrumbs.Item>
}

// The path to a page, ending in its title.
export function PageBreadcrumbs(props: Props) {
  const { t } = useTranslation('common')
  const { leading, folded, trailing } = foldPageCrumbs(props.parents)

  return <Breadcrumbs className='page-breadcrumbs min-w-0'>
    {leading.map(renderCrumb)}
    {Boolean(folded.length) && <Breadcrumbs.Item>{
      () => <>
        <Dropdown>
          <Tooltip delay={300}>
            <Button
              isIconOnly
              size='sm'
              variant='ghost'
              className='size-6 min-w-0'
              aria-label={t('page.moreBreadcrumbs')}
            >
              <LuEllipsis className='size-4' aria-hidden />
            </Button>
            <Tooltip.Content>
              <span>{t('page.moreBreadcrumbs')}</span>
            </Tooltip.Content>
          </Tooltip>
          <Dropdown.Popover placement='bottom start'>
            <Dropdown.Menu aria-label={t('page.moreBreadcrumbs')}>{
              folded.map((crumb) => <Dropdown.Item
                key={crumb.href}
                id={crumb.href}
                href={crumb.href}
                textValue={crumb.label}
              >
                <LuCornerLeftUp className='size-4' aria-hidden />
                <Label>{crumb.label}</Label>
              </Dropdown.Item>)
            }</Dropdown.Menu>
          </Dropdown.Popover>
        </Dropdown>
        <LuChevronRight className='breadcrumbs__separator' aria-hidden />
      </>
    }</Breadcrumbs.Item>}
    {trailing.map(renderCrumb)}
    <Breadcrumbs.Item>
      <span className='truncate'>{props.title}</span>
    </Breadcrumbs.Item>
  </Breadcrumbs>
}
