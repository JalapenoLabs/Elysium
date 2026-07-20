// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'
import type { OrgMember } from '../../api/routes/orgRoutes'

// Core
import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'

// UI
import { Accordion, AccordionItem, Button, Chip, Input } from '@heroui/react'
import { ProjectPanel } from './ProjectPanel'

// Utility
import { HTTPError } from 'ky'

// Misc
import { createProject } from '../../api/routes/projectRoutes'
import { listOrgMembers } from '../../api/routes/orgRoutes'
import { reloadProjects } from '../../hooks/useProjectsBootstrap'
import { UrlTree } from '../../urls'

const MANAGER_ROLES = [ 'owner', 'admin' ]

// The org's projects (docs/projects.md): the sub-tenant workspaces that own
// domains, Drives, and integrations. Create/rename/delete, domains, and
// per-project membership; each project expands into its panel. The roster
// itself lives in the projects slice (shared with the taskbar switcher), so
// every mutation here refreshes both surfaces.
export function Projects() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const navigate = useNavigate()

  const activeOrgId = useAppSelector((state) => state.sessionContext.activeOrgId)
  const orgs = useAppSelector((state) => state.sessionContext.orgs)
  const activeOrg = orgs.find((org) => org.id === activeOrgId)
  const canManage = Boolean(activeOrg && MANAGER_ROLES.includes(activeOrg.role))

  const projects = useAppSelector((state) => state.projects.projects)
  const activeProjectId = useAppSelector((state) => state.projects.activeProjectId)

  const [ orgMembers, setOrgMembers ] = useState<OrgMember[]>([])
  const [ newProjectName, setNewProjectName ] = useState('')
  const [ isCreating, setIsCreating ] = useState(false)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)

  useEffect(() => {
    if (!activeOrgId) {
      return
    }
    listOrgMembers(activeOrgId)
      .then((response) => setOrgMembers(response.members))
      .catch((error) => {
        console.debug('org roster unavailable for the member picker', { error })
      })
  }, [ activeOrgId ])

  function onForbidden() {
    navigate(UrlTree.accessDenied)
  }

  async function onCreate(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!activeOrgId || !newProjectName.trim()) {
      return
    }

    setIsCreating(true)
    setErrorMessage(null)
    try {
      await createProject(newProjectName.trim())
      setNewProjectName('')
      await reloadProjects(activeOrgId)
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 403) {
        onForbidden()
        return
      }
      console.debug('project creation failed', { error })
      setErrorMessage(t('auth:settings.projects.createFailed'))
    }
    finally {
      setIsCreating(false)
    }
  }

  return <section className='w-full'>
    <div className='relaxed'>
      <h2 className='text-2xl'>
        <strong>{ t('auth:settings.projects.title') }</strong>
      </h2>
      <p className='opacity-80'>{ t('auth:settings.projects.subtitle') }</p>
    </div>

    { canManage
      ? <form className='relaxed' onSubmit={onCreate}>
        <div className='level items-end'>
          <Input
            size='sm'
            className='w-full'
            label={t('auth:settings.projects.createLabel')}
            value={newProjectName}
            onChange={(event) => setNewProjectName(event.currentTarget.value)}
          />
          <Button
            size='sm'
            color='primary'
            variant='flat'
            type='submit'
            isDisabled={isCreating || !newProjectName.trim()}
            isLoading={isCreating}
          >
            <span>{ t('auth:settings.projects.createAction') }</span>
          </Button>
        </div>
      </form>
      : null }

    { errorMessage
      ? <p className='compact text-sm text-danger'>{ errorMessage }</p>
      : null }

    { projects.length
      ? <Accordion selectionMode='multiple' variant='bordered'>
        { projects.map((project) =>
          <AccordionItem
            key={project.id}
            aria-label={project.name}
            title={
              <span className='flex items-center gap-2'>
                <span>{ project.name }</span>
                <span className='text-xs opacity-50'>{ project.slug }</span>
                { project.id === activeProjectId
                  ? <Chip size='sm' variant='flat' color='primary'>{
                    t('auth:settings.projects.activeBadge')
                  }</Chip>
                  : null }
              </span>
            }
          >
            <ProjectPanel
              project={project}
              orgMembers={orgMembers}
              canManage={canManage}
              isLastProject={projects.length === 1}
              onForbidden={onForbidden}
              onChanged={() => {
                if (activeOrgId) {
                  reloadProjects(activeOrgId)
                }
              }}
            />
          </AccordionItem>,
        ) }
      </Accordion>
      : <p className='text-sm opacity-60'>{ t('auth:settings.projects.empty') }</p> }
  </section>
}
