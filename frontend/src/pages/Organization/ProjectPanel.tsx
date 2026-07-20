// Copyright © 2026 Jalapeno Labs

import type { OrgMember } from '../../api/routes/orgRoutes'
import type { ProjectMemberInfo } from '../../proto/generated/project_pb'
import type { ProjectView } from '../../store/projectsSlice'

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

// UI
import {
  Button,
  Chip,
  Input,
  Select,
  SelectItem,
  Tooltip,
  User,
} from '@heroui/react'
import { LuTrash2, LuX } from 'react-icons/lu'

// Misc
import { useConfirm } from '../../hooks/useConfirm'
import { ProjectRole } from '../../proto/generated/project_pb'
import {
  addProjectDomain,
  deleteProject,
  listProjectMembers,
  removeProjectDomain,
  removeProjectMember,
  setProjectMember,
  updateProject,
} from '../../api/routes/projectRoutes'

type Props = {
  project: ProjectView
  // The org roster, for the add-member picker.
  orgMembers: OrgMember[]
  canManage: boolean
  // Every org keeps at least one project; the delete control locks on the last.
  isLastProject: boolean
  // Bubbles a 403 to the page, which owns navigation.
  onForbidden: () => void
  onChanged: () => void
}

// Assignable project roles with their labels; UNSPECIFIED is not offered.
const assignableRoles = [
  { role: ProjectRole.MEMBER, labelKey: 'settings.projects.roles.member' },
  { role: ProjectRole.ADMIN, labelKey: 'settings.projects.roles.admin' },
  { role: ProjectRole.OWNER, labelKey: 'settings.projects.roles.owner' },
] as const

// One project's panel inside the Projects accordion: rename/slug, domains,
// the member roster with roles, and delete (guarded on the last project).
export function ProjectPanel(props: Props) {
  const { t } = useTranslation([ 'auth', 'common' ])
  const confirm = useConfirm()

  const [ members, setMembers ] = useState<ProjectMemberInfo[]>([])
  const [ name, setName ] = useState(props.project.name)
  const [ slug, setSlug ] = useState(props.project.slug)
  const [ newDomain, setNewDomain ] = useState('')
  const [ addUserId, setAddUserId ] = useState('')
  const [ isBusy, setIsBusy ] = useState(false)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)

  async function loadRoster() {
    try {
      const response = await listProjectMembers(props.project.id)
      setMembers(response.members)
    }
    catch (error) {
      console.debug('project roster unavailable', { error })
      setErrorMessage(t('auth:settings.projects.loadFailed'))
    }
  }

  useEffect(() => {
    loadRoster()
  }, [ props.project.id ])

  // Wraps a mutation with the shared busy/403/error handling.
  async function run(action: () => Promise<unknown>, refresh: () => Promise<void> | void) {
    setIsBusy(true)
    setErrorMessage(null)
    try {
      await action()
      await refresh()
    }
    catch (error) {
      const status = (error as { response?: Response }).response?.status
      if (status === 403) {
        props.onForbidden()
        return
      }
      if (status === 409) {
        setErrorMessage(t('auth:settings.projects.conflict'))
        return
      }
      console.debug('project update failed', { error })
      setErrorMessage(t('auth:settings.projects.updateFailed'))
    }
    finally {
      setIsBusy(false)
    }
  }

  const trimmedName = name.trim()
  const trimmedSlug = slug.trim()
  const hasIdentityEdit = (trimmedName && trimmedName !== props.project.name)
    || (trimmedSlug && trimmedSlug !== props.project.slug)

  const memberIds = new Set(members.map((member) => member.userId?.value))
  const addableMembers = props.orgMembers.filter((member) => !memberIds.has(member.user_id))

  return <div className='w-full'>
    { props.canManage
      ? <div className='compact level items-end'>
        <Input
          size='sm'
          className='w-full'
          label={t('auth:settings.projects.nameLabel')}
          value={name}
          onChange={(event) => setName(event.currentTarget.value)}
        />
        <Input
          size='sm'
          className='w-full'
          label={t('auth:settings.projects.slugLabel')}
          value={slug}
          onChange={(event) => setSlug(event.currentTarget.value)}
        />
        <Button
          size='sm'
          variant='flat'
          isDisabled={isBusy || !hasIdentityEdit}
          onPress={() => run(
            () => updateProject(props.project.id, { name: trimmedName, slug: trimmedSlug }),
            props.onChanged,
          )}
        >
          <span>{ t('auth:settings.projects.rename') }</span>
        </Button>
        <Tooltip
          content={t('auth:settings.projects.lastProject')}
          isDisabled={!props.isLastProject}
        >
          <div>
            <Button
              size='sm'
              variant='light'
              color='danger'
              isDisabled={isBusy || props.isLastProject}
              onPress={() => confirm({
                title: t('auth:settings.projects.delete'),
                message: t('auth:settings.projects.deleteConfirm', { name: props.project.name }),
                confirmLabel: t('auth:settings.projects.deleteAction'),
                confirmColor: 'danger',
                onConfirm: () => run(() => deleteProject(props.project.id), props.onChanged),
              })}
            >
              <LuTrash2 className='h-4 w-4' />
              <span>{ t('auth:settings.projects.delete') }</span>
            </Button>
          </div>
        </Tooltip>
      </div>
      : null }

    {/* Domains: the project owns them; mail/DNS status arrives with M6. */}
    <div className='compact'>
      <p className='compact text-sm font-medium opacity-70'>{
        t('auth:settings.projects.domainsTitle')
      }</p>
      <div className='compact flex flex-wrap items-center gap-2'>
        { props.project.domains.map((domain) =>
          <Chip
            key={domain}
            size='sm'
            variant='flat'
            endContent={props.canManage
              ? <button
                type='button'
                aria-label={t('auth:settings.projects.removeDomain', { domain })}
                className='opacity-60 hover:opacity-100'
                disabled={isBusy}
                onClick={() => run(() => removeProjectDomain(props.project.id, domain), props.onChanged)}
              >
                <LuX className='h-3 w-3' />
              </button>
              : null}
          >
            { domain }
          </Chip>,
        ) }
        { !props.project.domains.length
          ? <p className='text-sm opacity-60'>{ t('auth:settings.projects.noDomains') }</p>
          : null }
      </div>
      { props.canManage
        ? <div className='level items-end'>
          <Input
            size='sm'
            className='w-full'
            label={t('auth:settings.projects.domainLabel')}
            placeholder='example.com'
            value={newDomain}
            onChange={(event) => setNewDomain(event.currentTarget.value)}
          />
          <Button
            size='sm'
            variant='flat'
            isDisabled={isBusy || !newDomain.trim()}
            onPress={() => run(
              () => addProjectDomain(props.project.id, newDomain.trim().toLowerCase()),
              () => {
                setNewDomain('')
                props.onChanged()
              },
            )}
          >
            <span>{ t('auth:settings.projects.addDomain') }</span>
          </Button>
        </div>
        : null }
    </div>

    {/* Membership: full org members already reach every project; these are the
        explicit per-project roles (and the path guests use). */}
    <p className='compact text-sm font-medium opacity-70'>{
      t('auth:settings.projects.membersTitle')
    }</p>
    { members.map((member) => {
      const memberId = member.userId?.value ?? ''
      return <div key={memberId} className='compact level items-center'>
        <User
          name={member.displayName || member.email}
          description={member.email}
          avatarProps={{
            size: 'sm',
            name: member.displayName || member.email,
            src: member.avatarUrl || undefined,
          }}
        />
        { props.canManage
          ? <div className='flex items-center gap-2'>
            <Select
              size='sm'
              className='w-32'
              aria-label={t('auth:settings.projects.roleLabel')}
              selectedKeys={[ String(member.role) ]}
              onChange={(event) => {
                const role = Number(event.target.value) as ProjectRole
                run(() => setProjectMember(props.project.id, memberId, role), loadRoster)
              }}
            >
              { assignableRoles.map((option) =>
                <SelectItem key={String(option.role)}>
                  { t(`auth:${option.labelKey}`) }
                </SelectItem>,
              ) }
            </Select>
            <Button
              size='sm'
              variant='light'
              color='danger'
              isDisabled={isBusy}
              onPress={() => run(() => removeProjectMember(props.project.id, memberId), loadRoster)}
            >
              <span>{ t('auth:settings.projects.removeMember') }</span>
            </Button>
          </div>
          : <Chip size='sm' variant='flat'>{
            t(`auth:${assignableRoles.find((option) => option.role === member.role)?.labelKey
              ?? 'settings.projects.roles.member'}`)
          }</Chip> }
      </div>
    }) }
    { !members.length
      ? <p className='compact text-sm opacity-60'>{ t('auth:settings.projects.emptyMembers') }</p>
      : null }

    { props.canManage && addableMembers.length
      ? <div className='level items-end'>
        <Select
          size='sm'
          className='w-full'
          label={t('auth:settings.projects.addLabel')}
          selectedKeys={addUserId ? [ addUserId ] : []}
          onChange={(event) => setAddUserId(event.target.value)}
        >
          { addableMembers.map((member) =>
            <SelectItem key={member.user_id}>
              { member.display_name || member.email }
            </SelectItem>,
          ) }
        </Select>
        <Button
          size='sm'
          color='primary'
          variant='flat'
          isDisabled={isBusy || !addUserId}
          onPress={() => run(
            () => setProjectMember(props.project.id, addUserId, ProjectRole.MEMBER),
            async () => {
              setAddUserId('')
              await loadRoster()
            },
          )}
        >
          <span>{ t('auth:settings.projects.addAction') }</span>
        </Button>
      </div>
      : null }

    { errorMessage
      ? <p className='mt-2 text-sm text-danger'>{ errorMessage }</p>
      : null }
  </div>
}
