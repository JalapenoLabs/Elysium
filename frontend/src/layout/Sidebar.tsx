// Copyright © 2026 Jalapeno Labs

// Redux
import { useAppSelector } from '../store/hooks'
import { selectIsSidebarCollapsed } from '../store/sidebarSlice'

// User interface
import { SidebarCollapseButton } from './SidebarCollapseButton'
import { SidebarNavigation } from './SidebarNavigation'

// Below md the sidebar would take more than half a phone's width, so it gives way to the
// menu drawer in the topbar (MobileNavigation). From md up it collapses to its icons, and
// remembers that per browser (`startSidebarSync`). Collapsed, 4rem less its padding leaves
// exactly an icon and its link's padding, so the icons stay put while the width animates.
export function Sidebar() {
  const isCollapsed = useAppSelector(selectIsSidebarCollapsed)

  return <aside className={[
    'group/sidebar relative hidden shrink-0 flex-col border-r border-separator bg-sidebar px-2 py-3 md:flex',
    'transition-[width] duration-200 ease-out motion-reduce:transition-none',
    isCollapsed
      ? 'w-16'
      : 'w-60',
  ].join(' ')}>
    <SidebarCollapseButton />
    <SidebarNavigation isCollapsed={isCollapsed} />
  </aside>
}
