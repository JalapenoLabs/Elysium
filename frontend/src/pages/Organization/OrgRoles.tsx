// Copyright © 2026 Jalapeno Labs

import type { OrgCapability, OrgGrant, OrgMember } from '../../api/routes/orgRoutes'

// Core
import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'

// UI
import { Card, CardBody, Checkbox, Chip, User } from '@heroui/react'

// Utility
import { HTTPError } from 'ky'

// Misc
import { listOrgGrants, listOrgMembers, setOrgMemberGrants } from '../../api/routes/orgRoutes'
import { UrlTree } from '../../urls'

// The delegatable admin capabilities, in display order. Billing is deliberately
// not delegatable. Owners and admins hold all of these implicitly.
const CAPABILITIES: OrgCapability[] = [
  'manage_members',
  'manage_security',
  'manage_teams',
  'view_audit',
]

const MANAGER_ROLES = [ 'owner', 'admin' ]

// Delegated admin: grant individual capabilities to members so they can run one
// admin area without being a full admin. Owners and admins hold every
// capability implicitly (shown as a locked "Admin" state); regular members get
// a checkbox per capability. Owners/admins edit; the server is the authority.
export function OrgRoles() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const navigate = useNavigate()

  const activeOrgId = useAppSelector((state) => state.sessionContext.activeOrgId)
  const orgs = useAppSelector((state) => state.sessionContext.orgs)
  const activeOrg = orgs.find((org) => org.id === activeOrgId)
  const canManage = Boolean(activeOrg && MANAGER_ROLES.includes(activeOrg.role))

  const [ members, setMembers ] = useState<OrgMember[]>([])
  const [ grants, setGrants ] = useState<OrgGrant[]>([])
  const [ isLoading, setIsLoading ] = useState(true)
  const [ busyMemberId, setBusyMemberId ] = useState<string | null>(null)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)

  async function load(orgId: string) {
    try {
      const [ membersResponse, grantsResponse ] = await Promise.all([
        listOrgMembers(orgId),
        listOrgGrants(orgId),
      ])
      setMembers(membersResponse.members)
      setGrants(grantsResponse.grants)
    }
    catch (error) {
      console.debug('grants unavailable', { error })
      setErrorMessage(t('auth:orgRoles.loadFailed'))
    }
    finally {
      setIsLoading(false)
    }
  }

  useEffect(() => {
    if (!activeOrgId) {
      return
    }
    setIsLoading(true)
    load(activeOrgId)
  }, [ activeOrgId ])

  // The current capability set for a member, derived from the flat grant list.
  function capabilitiesFor(userId: string): Set<OrgCapability> {
    return new Set(
      grants
        .filter((grant) => grant.user_id === userId)
        .map((grant) => grant.capability),
    )
  }

  async function toggleCapability(member: OrgMember, capability: OrgCapability, granted: boolean) {
    if (!activeOrgId) {
      return
    }
    const current = capabilitiesFor(member.user_id)
    if (granted) {
      current.add(capability)
    }
    else {
      current.delete(capability)
    }

    setBusyMemberId(member.user_id)
    setErrorMessage(null)
    try {
      await setOrgMemberGrants(activeOrgId, member.user_id, [ ...current ])
      await load(activeOrgId)
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 403) {
        navigate(UrlTree.accessDenied)
        return
      }
      console.debug('grant update failed', { error })
      setErrorMessage(t('auth:orgRoles.updateFailed'))
    }
    finally {
      setBusyMemberId(null)
    }
  }

  return <section className='w-full'>
    <div className='relaxed'>
      <h2 className='text-2xl'>
        <strong>{ t('auth:orgRoles.title') }</strong>
      </h2>
      <p className='opacity-80'>{ t('auth:orgRoles.subtitle') }</p>
    </div>

    { errorMessage
      ? <p className='compact text-sm text-danger'>{ errorMessage }</p>
      : null }

    { isLoading
      ? <p className='opacity-60'>{ t('auth:orgRoles.loading') }</p>
      : members.map((member) => {
        const held = capabilitiesFor(member.user_id)
        const isImplicitAdmin = MANAGER_ROLES.includes(member.role)

        return <div key={member.user_id} className='compact'>
          <Card className='p-1'>
            <CardBody>
              <div className='level items-center'>
                <User
                  name={member.display_name || member.email}
                  description={member.email}
                  avatarProps={{
                    size: 'sm',
                    name: member.display_name || member.email,
                    src: member.avatar_url || undefined,
                  }}
                />
                { isImplicitAdmin
                  ? <Chip size='sm' variant='flat' color='primary'>
                    { t('auth:orgRoles.allCapabilities') }
                  </Chip>
                  : null }
              </div>

              {/* Implicit admins already hold every capability, so there's
                  nothing to toggle for them. */}
              { !isImplicitAdmin
                ? <div className='mt-3 flex flex-wrap gap-x-6 gap-y-2'>
                  { CAPABILITIES.map((capability) =>
                    <Checkbox
                      key={capability}
                      size='sm'
                      isSelected={held.has(capability)}
                      isDisabled={!canManage || busyMemberId === member.user_id}
                      onValueChange={(value) => toggleCapability(member, capability, value)}
                    >
                      { t(`auth:orgRoles.capabilities.${capability}`) }
                    </Checkbox>,
                  ) }
                </div>
                : null }
            </CardBody>
          </Card>
        </div>
      }) }
  </section>
}
