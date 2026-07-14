// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'
import type { Team } from '../../api/routes/teamRoutes'
import type { OrgMember } from '../../api/routes/orgRoutes'

// Core
import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'

// UI
import { Accordion, AccordionItem, Button, Input } from '@heroui/react'
import { TeamPanel } from './TeamPanel'

// Utility
import { HTTPError } from 'ky'

// Misc
import { createTeam, listTeams } from '../../api/routes/teamRoutes'
import { listOrgMembers } from '../../api/routes/orgRoutes'
import { UrlTree } from '../../urls'

const MANAGER_ROLES = [ 'owner', 'admin' ]

// The active org's teams: create, rename, delete, and per-team rosters (each
// team expands into its panel). Members view; owners and admins manage.
export function Teams() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const navigate = useNavigate()

  const activeOrgId = useAppSelector((state) => state.sessionContext.activeOrgId)
  const orgs = useAppSelector((state) => state.sessionContext.orgs)
  const activeOrg = orgs.find((org) => org.id === activeOrgId)
  const canManage = Boolean(activeOrg && MANAGER_ROLES.includes(activeOrg.role))

  const [ teams, setTeams ] = useState<Team[]>([])
  const [ orgMembers, setOrgMembers ] = useState<OrgMember[]>([])
  const [ newTeamName, setNewTeamName ] = useState('')
  const [ isLoading, setIsLoading ] = useState(true)
  const [ isCreating, setIsCreating ] = useState(false)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)

  async function loadTeams(orgId: string) {
    try {
      const [ teamsResponse, membersResponse ] = await Promise.all([
        listTeams(orgId),
        listOrgMembers(orgId),
      ])
      setTeams(teamsResponse.teams)
      setOrgMembers(membersResponse.members)
    }
    catch (error) {
      console.debug('teams unavailable', { error })
      setErrorMessage(t('auth:settings.teams.loadFailed'))
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
    loadTeams(activeOrgId)
  }, [ activeOrgId ])

  function onForbidden() {
    navigate(UrlTree.accessDenied)
  }

  async function onCreate(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!activeOrgId || !newTeamName.trim()) {
      return
    }

    setIsCreating(true)
    setErrorMessage(null)
    try {
      await createTeam(activeOrgId, newTeamName.trim())
      setNewTeamName('')
      await loadTeams(activeOrgId)
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 403) {
        onForbidden()
        return
      }
      console.debug('team creation failed', { error })
      setErrorMessage(t('auth:settings.teams.createFailed'))
    }
    finally {
      setIsCreating(false)
    }
  }

  return <section className='w-full'>
    <div className='relaxed'>
      <h2 className='text-2xl'>
        <strong>{ t('auth:settings.teams.title') }</strong>
      </h2>
      <p className='opacity-80'>{ t('auth:settings.teams.subtitle') }</p>
    </div>

    { canManage
      ? <form className='relaxed' onSubmit={onCreate}>
        <div className='level items-end'>
          <Input
            size='sm'
            className='w-full'
            label={t('auth:settings.teams.createLabel')}
            value={newTeamName}
            onChange={(event) => setNewTeamName(event.currentTarget.value)}
          />
          <Button
            type='submit'
            size='sm'
            color='primary'
            isLoading={isCreating}
            isDisabled={isCreating || !newTeamName.trim()}
          >
            <span>{ t('auth:settings.teams.createAction') }</span>
          </Button>
        </div>
      </form>
      : null }

    { errorMessage
      ? <p className='compact text-sm text-danger'>{ errorMessage }</p>
      : null }

    { isLoading
      ? <p className='opacity-60'>{ t('auth:settings.teams.loading') }</p>
      : teams.length
        ? <Accordion variant='splitted'>
          { teams.map((team) =>
            <AccordionItem key={team.id} title={team.name}>
              <TeamPanel
                team={team}
                orgMembers={orgMembers}
                canManage={canManage}
                onForbidden={onForbidden}
                onChanged={() => activeOrgId && loadTeams(activeOrgId)}
              />
            </AccordionItem>,
          ) }
        </Accordion>
        : <p className='opacity-60'>{ t('auth:settings.teams.none') }</p> }
  </section>
}
