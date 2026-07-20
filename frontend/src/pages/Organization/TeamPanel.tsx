// Copyright © 2026 Jalapeno Labs

import type { Team, TeamMember } from '../../api/routes/teamRoutes'
import type { OrgMember } from '../../api/routes/orgRoutes'

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

// UI
import {
  Button,
  Input,
  Select,
  SelectItem,
  User,
} from '@heroui/react'
import { LuTrash2 } from 'react-icons/lu'

// Misc
import { useConfirm } from '../../hooks/useConfirm'

// Misc
import {
  addTeamMember,
  deleteTeam,
  listTeamMembers,
  removeTeamMember,
  renameTeam,
} from '../../api/routes/teamRoutes'

type Props = {
  team: Team
  // The org roster, for the add-member picker.
  orgMembers: OrgMember[]
  canManage: boolean
  // Bubbles a 403 (or reload need) to the page, which owns navigation/reloads.
  onForbidden: () => void
  onChanged: () => void
}

// One team's panel: rename, delete, and the roster with add/remove. Rendered
// inside the Teams accordion; loads its roster when it first mounts (i.e. when
// the accordion opens it).
export function TeamPanel(props: Props) {
  const { t } = useTranslation([ 'auth', 'common' ])
  const confirm = useConfirm()

  const [ members, setMembers ] = useState<TeamMember[]>([])
  const [ name, setName ] = useState(props.team.name)
  const [ addUserId, setAddUserId ] = useState('')
  const [ isBusy, setIsBusy ] = useState(false)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)

  async function loadRoster() {
    try {
      const response = await listTeamMembers(props.team.id)
      setMembers(response.members)
    }
    catch (error) {
      console.debug('team roster unavailable', { error })
      setErrorMessage(t('auth:settings.teams.loadFailed'))
    }
  }

  useEffect(() => {
    loadRoster()
  }, [ props.team.id ])

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
      console.debug('team update failed', { error })
      setErrorMessage(t('auth:settings.teams.updateFailed'))
    }
    finally {
      setIsBusy(false)
    }
  }

  const memberIds = new Set(members.map((member) => member.user_id))
  const addableMembers = props.orgMembers.filter((member) => !memberIds.has(member.user_id))

  return <div className='w-full'>
    { props.canManage
      ? <div className='compact level items-end'>
        <Input
          size='sm'
          className='w-full'
          label={t('auth:settings.teams.nameLabel')}
          value={name}
          onChange={(event) => setName(event.currentTarget.value)}
        />
        <Button
          size='sm'
          variant='flat'
          isDisabled={isBusy || !name.trim() || name.trim() === props.team.name}
          onPress={() => run(() => renameTeam(props.team.id, name.trim()), props.onChanged)}
        >
          <span>{ t('auth:settings.teams.rename') }</span>
        </Button>
        <Button
          size='sm'
          variant='light'
          color='danger'
          isDisabled={isBusy}
          onPress={() => confirm({
            title: t('auth:settings.teams.delete'),
            message: t('auth:settings.teams.deleteConfirm', { name: props.team.name }),
            confirmLabel: t('auth:settings.teams.deleteAction'),
            confirmColor: 'danger',
            onConfirm: () => run(() => deleteTeam(props.team.id), props.onChanged),
          })}
        >
          <LuTrash2 className='h-4 w-4' />
          <span>{ t('auth:settings.teams.delete') }</span>
        </Button>
      </div>
      : null }

    { members.map((member) =>
      <div key={member.user_id} className='compact level items-center'>
        <User
          name={member.display_name || member.email}
          description={member.email}
          avatarProps={{
            size: 'sm',
            name: member.display_name || member.email,
            src: member.avatar_url || undefined,
          }}
        />
        { props.canManage
          ? <Button
            size='sm'
            variant='light'
            color='danger'
            isDisabled={isBusy}
            onPress={() => run(() => removeTeamMember(props.team.id, member.user_id), loadRoster)}
          >
            <span>{ t('auth:settings.teams.removeMember') }</span>
          </Button>
          : null }
      </div>,
    ) }
    { !members.length
      ? <p className='compact text-sm opacity-60'>{ t('auth:settings.teams.empty') }</p>
      : null }

    { props.canManage && addableMembers.length
      ? <div className='level items-end'>
        <Select
          size='sm'
          className='w-full'
          label={t('auth:settings.teams.addLabel')}
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
            () => addTeamMember(props.team.id, addUserId),
            async () => {
              setAddUserId('')
              await loadRoster()
            },
          )}
        >
          <span>{ t('auth:settings.teams.addAction') }</span>
        </Button>
      </div>
      : null }

    { errorMessage
      ? <p className='mt-2 text-sm text-danger'>{ errorMessage }</p>
      : null }
  </div>
}
