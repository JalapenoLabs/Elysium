// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Link } from 'react-aria-components'
import { SidebarTooltip } from './SidebarTooltip'

// Misc
import { UrlTree } from '../urls'

type Props = {
  isCollapsed: boolean
  onNavigate?: () => void
}

// The logo at the foot of the sidebar, the full width of it, linking to the main page
// (`~/.claude/docs/ux.md`, Sidebars). Collapsed, only the mark shows: it sits in the same
// column as the link icons, so collapsing clips the name without moving the mark.
export function SidebarBrand(props: Props) {
  const { t } = useTranslation('common')
  const name = t('brand.name')

  return <div className='mt-2 border-t border-separator pt-2'>
    <SidebarTooltip label={name} isEnabled={props.isCollapsed}>
      <Link
        href={UrlTree.root}
        onPress={props.onNavigate}
        className={[
          'flex w-full items-center gap-3 overflow-hidden rounded-lg px-2 py-3 outline-none',
          'transition-colors duration-200 ease-out motion-reduce:transition-none',
          'data-hovered:bg-default data-focus-visible:ring-2 data-focus-visible:ring-focus',
        ].join(' ')}
      >
        <span
          aria-hidden
          className='grid size-8 shrink-0 place-items-center rounded-lg bg-accent font-bold text-accent-foreground'
        >
          E
        </span>
        <span className={[
          'whitespace-nowrap text-lg font-semibold',
          'transition-opacity duration-200 ease-out motion-reduce:transition-none',
          props.isCollapsed
            ? 'opacity-0'
            : 'opacity-100',
        ].join(' ')}>{
          name
        }</span>
      </Link>
    </SidebarTooltip>
  </div>
}
