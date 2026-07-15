// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'

// UI
import {
  Avatar,
  Button,
  Dropdown,
  DropdownItem,
  DropdownMenu,
  DropdownSection,
  DropdownTrigger,
} from '@heroui/react'
import { LuBuilding2, LuCheck, LuPlus, LuSettings } from 'react-icons/lu'
import { CreateOrgModal } from './CreateOrgModal'

// Misc
import { switchOrg } from '../../api/routes/sessionRoutes'
import { resolveSession } from '../../auth/session'
import { UrlTree } from '../../urls'

// The taskbar's organization drop-up: shows the active org and switches between
// every org the active account belongs to, plus adding a new one. It renders
// straight from the session-context slice, which the `/context` bootstrap seeds
// and the realtime stream keeps live (an admin renaming the org updates every
// member's taskbar). See docs/identity.md and docs/realtime.md.
export function OrgSwitcher() {
  const { t } = useTranslation('common')
  const navigate = useNavigate()

  const orgs = useAppSelector((state) => state.sessionContext.orgs)
  const activeOrgId = useAppSelector((state) => state.sessionContext.activeOrgId)

  const [ isCreateOpen, setIsCreateOpen ] = useState(false)

  // Switching happens server-side on the session; re-resolving reseeds the
  // context (active org, orgs, accounts) in one call, then home re-renders in
  // the new org's context.
  async function onSwitchOrg(orgId: string) {
    if (orgId === activeOrgId) {
      return
    }
    try {
      await switchOrg(orgId)
      await resolveSession()
      navigate(UrlTree.home)
    }
    catch (error) {
      console.debug('org switch failed', { error })
    }
  }

  const activeOrg = orgs.find((org) => org.id === activeOrgId)

  return <>
    <CreateOrgModal
      isOpen={isCreateOpen}
      onOpenChange={setIsCreateOpen}
    />
    <Dropdown placement='top-start'>
      <DropdownTrigger>
        <Button variant='light'>
          {/* The org's own logo when it has one; the building icon otherwise. */}
          { activeOrg?.logoUrl
            ? <Avatar size='sm' radius='sm' className='h-5 w-5' src={activeOrg.logoUrl} />
            : <LuBuilding2 className='h-4 w-4' /> }
          {/* The wordmark is the white-label brand text; the org name backs it up. */}
          <span>{ activeOrg?.wordmark || activeOrg?.name || t('org.switcherFallback') }</span>
        </Button>
      </DropdownTrigger>
      <DropdownMenu aria-label={t('org.menuLabel')}>
        <DropdownSection title={t('org.sectionTitle')} showDivider>
          { orgs.map((org) =>
            <DropdownItem
              key={org.id}
              textValue={org.name}
              endContent={org.id === activeOrgId ? <LuCheck className='h-4 w-4' /> : null}
              onPress={() => onSwitchOrg(org.id)}
            >
              <div>
                <p>{ org.name }</p>
                <p className='text-xs opacity-60'>{ org.role }</p>
              </div>
            </DropdownItem>,
          ) }
        </DropdownSection>
        <DropdownSection>
          <DropdownItem
            key='org-settings'
            startContent={<LuSettings className='h-4 w-4' />}
            onPress={() => navigate(UrlTree.organization)}
          >
            { t('org.settings') }
          </DropdownItem>
          <DropdownItem
            key='add-org'
            startContent={<LuPlus className='h-4 w-4' />}
            onPress={() => setIsCreateOpen(true)}
          >
            { t('org.addOrganization') }
          </DropdownItem>
        </DropdownSection>
      </DropdownMenu>
    </Dropdown>
  </>
}
