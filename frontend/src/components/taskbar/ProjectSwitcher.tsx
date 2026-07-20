// Copyright © 2026 Jalapeno Labs

// Core
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// Redux
import { dispatch } from '../../store'
import { useAppSelector } from '../../store/hooks'
import { activeProjectChanged, activeProjectStorageKey } from '../../store/projectsSlice'

// UI
import {
  Button,
  Dropdown,
  DropdownItem,
  DropdownMenu,
  DropdownSection,
  DropdownTrigger,
} from '@heroui/react'
import { LuCheck, LuLayers, LuPlus, LuSettings } from 'react-icons/lu'

// Misc
import { usePrompt } from '../../hooks/usePrompt'
import { createProject } from '../../api/routes/projectRoutes'
import { reloadProjects } from '../../hooks/useProjectsBootstrap'
import { UrlTree } from '../../urls'

const MANAGER_ROLES = [ 'owner', 'admin' ]

// The taskbar's project drop-up, one tier below the org switcher
// (docs/projects.md): the org is who you are, the project is where you are
// working. Switching is a client-side viewing preference (remembered per org);
// project-scoped surfaces (Drives, mail, integrations) follow it as they land.
export function ProjectSwitcher() {
  const { t } = useTranslation('common')
  const navigate = useNavigate()
  const prompt = usePrompt()

  const projects = useAppSelector((state) => state.projects.projects)
  const activeProjectId = useAppSelector((state) => state.projects.activeProjectId)
  const isLoaded = useAppSelector((state) => state.projects.isLoaded)

  const orgs = useAppSelector((state) => state.sessionContext.orgs)
  const activeOrgId = useAppSelector((state) => state.sessionContext.activeOrgId)
  const activeOrg = orgs.find((org) => org.id === activeOrgId)
  const canManage = Boolean(activeOrg && MANAGER_ROLES.includes(activeOrg.role))

  if (!isLoaded || !activeOrgId) {
    return null
  }

  function onSwitchProject(projectId: string) {
    dispatch(activeProjectChanged(projectId))
    localStorage.setItem(activeProjectStorageKey(activeOrgId!), projectId)
  }

  function onCreateProject() {
    prompt({
      title: t('project.createTitle'),
      inputProps: { label: t('project.nameLabel') },
      validate: (value) => {
        if (!value.trim()) {
          return t('project.nameRequired')
        }
        return null
      },
      confirmLabel: t('project.createAction'),
      onConfirm: async (value) => {
        const project = await createProject(value.trim())
        await reloadProjects(activeOrgId!)
        const projectId = project.id?.value
        if (projectId) {
          onSwitchProject(projectId)
        }
      },
    })
  }

  const activeProject = projects.find((project) => project.id === activeProjectId)

  return <Dropdown placement='top-start'>
    <DropdownTrigger>
      <Button variant='light'>
        <LuLayers className='h-4 w-4' />
        <span>{ activeProject?.name ?? t('project.switcherFallback') }</span>
      </Button>
    </DropdownTrigger>
    <DropdownMenu aria-label={t('project.menuLabel')}>
      <DropdownSection title={t('project.sectionTitle')} showDivider>
        { projects.map((project) =>
          <DropdownItem
            key={project.id}
            textValue={project.name}
            endContent={project.id === activeProjectId ? <LuCheck className='h-4 w-4' /> : null}
            onPress={() => onSwitchProject(project.id)}
          >
            <div>
              <p>{ project.name }</p>
              <p className='text-xs opacity-60'>{ project.slug }</p>
            </div>
          </DropdownItem>,
        ) }
      </DropdownSection>
      <DropdownSection>
        <DropdownItem
          key='project-settings'
          startContent={<LuSettings className='h-4 w-4' />}
          onPress={() => navigate(UrlTree.organizationProjects)}
        >
          { t('project.settings') }
        </DropdownItem>
        { canManage
          ? <DropdownItem
            key='add-project'
            startContent={<LuPlus className='h-4 w-4' />}
            onPress={onCreateProject}
          >
            { t('project.addProject') }
          </DropdownItem>
          : null }
      </DropdownSection>
    </DropdownMenu>
  </Dropdown>
}
