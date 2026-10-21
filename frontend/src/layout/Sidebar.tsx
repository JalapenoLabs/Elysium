// Copyright © 2026 Jalapeno Labs

// User interface
import { SidebarNavigation } from './SidebarNavigation'

// Below md the sidebar would take more than half a phone's width, so it gives way to the
// menu drawer in the topbar (MobileNavigation).
export function Sidebar() {
  return <aside className='hidden w-56 shrink-0 flex-col border-r border-separator bg-sidebar px-3 py-4 md:flex'>
    <SidebarNavigation />
  </aside>
}
