// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../store/hooks'
import { selectIsSidebarCollapsed, sidebarCollapsedChanged } from '../store/sidebarSlice'

// User interface
import { CollapseEdgeButton } from '../components/CollapseEdgeButton'

// Collapses the sidebar. It shows while the mouse is anywhere over the sidebar (the parent is
// `group/sidebar`), and sits level with the middle of the topbar, beside the account menu.
export function SidebarCollapseButton() {
  const { t } = useTranslation('navigation')
  const dispatch = useAppDispatch()
  const isCollapsed = useAppSelector(selectIsSidebarCollapsed)

  return <CollapseEdgeButton
    isCollapsed={isCollapsed}
    collapseLabel={t('sidebar.collapse')}
    expandLabel={t('sidebar.expand')}
    onToggle={() => dispatch(sidebarCollapsedChanged(!isCollapsed))}
    className='top-5 group-hover/sidebar:opacity-100'
  />
}
