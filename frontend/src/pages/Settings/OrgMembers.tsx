// Copyright © 2026 Jalapeno Labs

import type { OrgMember } from '../../api/routes/orgRoutes'

// Core
import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'

// UI
import {
  Button,
  Card,
  CardBody,
  Chip,
  Popover,
  PopoverContent,
  PopoverTrigger,
  Select,
  SelectItem,
  User,
} from '@heroui/react'

// Utility
import { HTTPError } from 'ky'

// Misc
import { listOrgMembers, removeOrgMember, updateOrgMemberRole } from '../../api/routes/orgRoutes'
import { subscribeRealtime } from '../../realtime/client'
import { UrlTree } from '../../urls'

// Roles a manager can grant. Ownership never changes hands here, so it is a
// chip, not an option; the server enforces the same rules (rank, no self-edit).
const GRANTABLE_ROLES = [ 'admin', 'member' ] as const
const MANAGER_ROLES = [ 'owner', 'admin' ]

// The active organization's roster: every member with their role. Admins and
// owners change roles or remove members; everyone else just sees the team.
// Membership changes push a `session.changed` nudge, so the roster reloads
// itself when someone else edits it.
export function OrgMembers() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const navigate = useNavigate()

  const activeOrgId = useAppSelector((state) => state.sessionContext.activeOrgId)
  const orgs = useAppSelector((state) => state.sessionContext.orgs)
  const currentUserId = useAppSelector((state) => state.currentUser.user?.id)

  const activeOrg = orgs.find((org) => org.id === activeOrgId)
  const canManage = Boolean(activeOrg && MANAGER_ROLES.includes(activeOrg.role))

  const [ members, setMembers ] = useState<OrgMember[]>([])
  const [ isLoading, setIsLoading ] = useState(true)
  const [ busyMemberId, setBusyMemberId ] = useState<string | null>(null)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)

  async function loadMembers(orgId: string) {
    try {
      const response = await listOrgMembers(orgId)
      setMembers(response.members)
    }
    catch (error) {
      console.debug('member list unavailable', { error })
      setErrorMessage(t('auth:settings.members.loadFailed'))
    }
    finally {
      setIsLoading(false)
    }
  }

  // Load per org, and reload whenever membership changes anywhere (the server
  // nudges `session.changed` on role changes and removals).
  useEffect(() => {
    if (!activeOrgId) {
      return undefined
    }
    setIsLoading(true)
    loadMembers(activeOrgId)
    return subscribeRealtime('session.changed', () => {
      loadMembers(activeOrgId)
    })
  }, [ activeOrgId ])

  async function onMemberAction(memberId: string, action: () => Promise<Response>) {
    if (!activeOrgId) {
      return
    }
    setBusyMemberId(memberId)
    setErrorMessage(null)
    try {
      await action()
      await loadMembers(activeOrgId)
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 403) {
        navigate(UrlTree.accessDenied)
        return
      }
      console.debug('member update failed', { error })
      setErrorMessage(t('auth:settings.members.updateFailed'))
    }
    finally {
      setBusyMemberId(null)
    }
  }

  // A row is editable only when the viewer manages the org, the target isn't
  // the owner, and it isn't the viewer themselves (matching the server rules).
  function isEditable(member: OrgMember) {
    return canManage && member.role !== 'owner' && member.user_id !== currentUserId
  }

  return <section className='w-full'>
    <div className='relaxed'>
      <h2 className='text-2xl'>
        <strong>{ t('auth:settings.members.title') }</strong>
      </h2>
      <p className='opacity-80'>{ t('auth:settings.members.subtitle') }</p>
    </div>

    { errorMessage
      ? <p className='compact text-sm text-danger'>{ errorMessage }</p>
      : null }

    { isLoading
      ? <p className='opacity-60'>{ t('auth:settings.members.loading') }</p>
      : members.map((member) =>
        <div key={member.user_id} className='compact'>
          <Card className='p-1'>
            <CardBody className='level items-center'>
              <User
                name={member.display_name || member.email}
                description={member.email}
                avatarProps={{
                  size: 'sm',
                  name: member.display_name || member.email,
                  src: member.avatar_url || undefined,
                }}
              />
              <div className='level-right'>
                { isEditable(member)
                  ? <>
                    <Select
                      size='sm'
                      className='w-36'
                      aria-label={t('auth:settings.members.roleLabel')}
                      isDisabled={busyMemberId === member.user_id}
                      selectedKeys={[ member.role ]}
                      onChange={(event) => {
                        const role = event.target.value
                        if (role && role !== member.role) {
                          onMemberAction(member.user_id, () =>
                            updateOrgMemberRole(activeOrgId ?? '', member.user_id, role))
                        }
                      }}
                    >
                      { GRANTABLE_ROLES.map((role) =>
                        <SelectItem key={role}>{ t(`auth:settings.members.roles.${role}`) }</SelectItem>,
                      ) }
                    </Select>
                    <Popover placement='top-end'>
                      <PopoverTrigger>
                        <Button
                          size='sm'
                          variant='light'
                          color='danger'
                          isDisabled={busyMemberId === member.user_id}
                        >
                          <span>{ t('auth:settings.members.remove') }</span>
                        </Button>
                      </PopoverTrigger>
                      <PopoverContent className='p-4'>
                        <p className='compact text-sm'>
                          { t('auth:settings.members.removeConfirm', {
                            name: member.display_name || member.email,
                          }) }
                        </p>
                        <Button
                          size='sm'
                          color='danger'
                          className='w-full'
                          isLoading={busyMemberId === member.user_id}
                          onPress={() =>
                            onMemberAction(member.user_id, () =>
                              removeOrgMember(activeOrgId ?? '', member.user_id))}
                        >
                          <span>{ t('auth:settings.members.removeAction') }</span>
                        </Button>
                      </PopoverContent>
                    </Popover>
                  </>
                  : <Chip size='sm' variant='flat'>
                    { t(`auth:settings.members.roles.${member.role}`, member.role) }
                  </Chip>
                }
              </div>
            </CardBody>
          </Card>
        </div>,
      ) }
  </section>
}
