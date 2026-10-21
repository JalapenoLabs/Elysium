// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Drawer, Tooltip, useOverlayState } from '@heroui/react'
import { LuMenu } from 'react-icons/lu'
import { SidebarNavigation } from './SidebarNavigation'

// The primary navigation below md, where the sidebar is hidden: a menu button in the topbar
// that opens the same links in a drawer from the left. Following a link closes it.
export function MobileNavigation() {
  const { t } = useTranslation('navigation')
  const drawer = useOverlayState()

  return <div className='md:hidden'>
    <Tooltip delay={300}>
      <Button
        isIconOnly
        variant='ghost'
        aria-label={t('topbar.openNavigation')}
        onPress={drawer.open}
      >
        <LuMenu className='size-5' aria-hidden />
      </Button>
      <Tooltip.Content>
        <span>{
          t('topbar.openNavigation')
        }</span>
      </Tooltip.Content>
    </Tooltip>
    {/* Controlled, so the root trigger wrapper is skipped, as in CreateSessionModal. */}
    <Drawer.Backdrop isOpen={drawer.isOpen} onOpenChange={drawer.setOpen}>
      <Drawer.Content placement='left'>
        <Drawer.Dialog aria-label={t('primary.label')} className='w-64 bg-sidebar px-3 py-4'>
          <SidebarNavigation onNavigate={drawer.close} />
        </Drawer.Dialog>
      </Drawer.Content>
    </Drawer.Backdrop>
  </div>
}
