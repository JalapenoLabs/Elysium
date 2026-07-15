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
  Dropdown,
  DropdownItem,
  DropdownMenu,
  DropdownTrigger,
  Modal,
  ModalBody,
  ModalContent,
  ModalFooter,
  ModalHeader,
  Select,
  SelectItem,
  User,
} from '@heroui/react'
import { LuEllipsisVertical } from 'react-icons/lu'

// Utility
import { HTTPError } from 'ky'

// Misc
import {
  listOrgMembers,
  reactivateOrgMember,
  removeOrgMember,
  suspendOrgMember,
  transferOrgOwnership,
  updateOrgMemberRole,
} from '../../api/routes/orgRoutes'
import { subscribeRealtime } from '../../realtime/client'
import { UrlTree } from '../../urls'

// Roles a manager can grant, lowest last. Ownership changes hands only via the
// explicit transfer action, so `owner` is never a grantable role here.
const GRANTABLE_ROLES = [ 'admin', 'member', 'guest' ] as const
const MANAGER_ROLES = [ 'owner', 'admin' ]

// The active organization's roster with lifecycle management: role changes,
// suspend/reactivate, ownership transfer, and removal. Admins and owners
// manage; everyone else just sees the team. Membership changes push a
// `session.changed` nudge, so the roster reloads itself when edited elsewhere.
export function OrgMembers() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const navigate = useNavigate()

  const activeOrgId = useAppSelector((state) => state.sessionContext.activeOrgId)
  const orgs = useAppSelector((state) => state.sessionContext.orgs)
  const currentUserId = useAppSelector((state) => state.currentUser.user?.id)

  const activeOrg = orgs.find((org) => org.id === activeOrgId)
  const canManage = Boolean(activeOrg && MANAGER_ROLES.includes(activeOrg.role))
  const isOwner = activeOrg?.role === 'owner'

  const [ members, setMembers ] = useState<OrgMember[]>([])
  const [ isLoading, setIsLoading ] = useState(true)
  const [ busyMemberId, setBusyMemberId ] = useState<string | null>(null)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)
  // A pending irreversible action awaiting confirmation (remove, transfer).
  const [ pendingConfirm, setPendingConfirm ] = useState<
    { title: string, body: string, actionLabel: string, run:() => void } | null
  >(null)

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
    <Modal isOpen={Boolean(pendingConfirm)} onOpenChange={() => setPendingConfirm(null)}>
      <ModalContent>
        <ModalHeader>{ pendingConfirm?.title }</ModalHeader>
        <ModalBody>
          <p className='opacity-80'>{ pendingConfirm?.body }</p>
        </ModalBody>
        <ModalFooter>
          <Button variant='light' onPress={() => setPendingConfirm(null)}>
            <span>{ t('common:actions.cancel') }</span>
          </Button>
          <Button
            color='danger'
            onPress={() => {
              pendingConfirm?.run()
              setPendingConfirm(null)
            }}
          >
            <span>{ pendingConfirm?.actionLabel }</span>
          </Button>
        </ModalFooter>
      </ModalContent>
    </Modal>

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
              <div className='flex items-center gap-2'>
                <User
                  name={member.display_name || member.email}
                  description={member.email}
                  avatarProps={{
                    size: 'sm',
                    name: member.display_name || member.email,
                    src: member.avatar_url || undefined,
                  }}
                />
                { member.suspended
                  ? <Chip size='sm' variant='flat' color='warning'>
                    { t('auth:settings.members.suspended') }
                  </Chip>
                  : null }
              </div>
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
                    <Dropdown placement='bottom-end'>
                      <DropdownTrigger>
                        <Button
                          isIconOnly
                          size='sm'
                          variant='light'
                          aria-label={t('auth:settings.members.actions')}
                          isDisabled={busyMemberId === member.user_id}
                        >
                          <LuEllipsisVertical className='h-4 w-4' />
                        </Button>
                      </DropdownTrigger>
                      <DropdownMenu aria-label={t('auth:settings.members.actions')}>
                        { member.suspended
                          ? <DropdownItem
                            key='reactivate'
                            onPress={() =>
                              onMemberAction(member.user_id, () =>
                                reactivateOrgMember(activeOrgId ?? '', member.user_id))}
                          >
                            { t('auth:settings.members.reactivate') }
                          </DropdownItem>
                          : <DropdownItem
                            key='suspend'
                            onPress={() =>
                              onMemberAction(member.user_id, () =>
                                suspendOrgMember(activeOrgId ?? '', member.user_id))}
                          >
                            { t('auth:settings.members.suspend') }
                          </DropdownItem>
                        }
                        { isOwner
                          ? <DropdownItem
                            key='transfer'
                            onPress={() => setPendingConfirm({
                              title: t('auth:settings.members.transferOwnership'),
                              body: t('auth:settings.members.transferConfirm', {
                                name: member.display_name || member.email,
                              }),
                              actionLabel: t('auth:settings.members.transferAction'),
                              run: () => onMemberAction(member.user_id, () =>
                                transferOrgOwnership(activeOrgId ?? '', member.user_id)),
                            })}
                          >
                            { t('auth:settings.members.transferOwnership') }
                          </DropdownItem>
                          : null }
                        <DropdownItem
                          key='remove'
                          color='danger'
                          className='text-danger'
                          onPress={() => setPendingConfirm({
                            title: t('auth:settings.members.remove'),
                            body: t('auth:settings.members.removeConfirm', {
                              name: member.display_name || member.email,
                            }),
                            actionLabel: t('auth:settings.members.removeAction'),
                            run: () => onMemberAction(member.user_id, () =>
                              removeOrgMember(activeOrgId ?? '', member.user_id)),
                          })}
                        >
                          { t('auth:settings.members.remove') }
                        </DropdownItem>
                      </DropdownMenu>
                    </Dropdown>
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
