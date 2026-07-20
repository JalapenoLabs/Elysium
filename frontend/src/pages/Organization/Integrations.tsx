// Copyright © 2026 Jalapeno Labs

import type { DriveView } from '../../drive/driveView'
import type { Integration } from '../../proto/generated/integration_pb'

// Core
import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'

// UI
import {
  Button,
  Chip,
  Dropdown,
  DropdownItem,
  DropdownMenu,
  DropdownTrigger,
  Select,
  SelectItem,
} from '@heroui/react'
import {
  LuContainer,
  LuEllipsisVertical,
  LuGitBranch,
  LuHardHat,
  LuMail,
  LuPackage,
  LuPlug,
  LuPlus,
} from 'react-icons/lu'
import { IntegrationModal } from './IntegrationModal'
import { EmptyState } from '../../components/states/EmptyState'
import { LoadingState } from '../../components/states/LoadingState'
import { StatusIndicator } from '../../components/status/StatusIndicator'

// Utility
import { HTTPError } from 'ky'

// Misc
import { useConfirm } from '../../hooks/useConfirm'
import {
  deleteIntegration,
  listIntegrations,
  startIntegration,
  stopIntegration,
} from '../../api/routes/integrationRoutes'
import { listDrives } from '../../api/routes/driveRoutes'
import { toDriveView } from '../../drive/driveView'
import {
  integrationKindMeta,
  reconcileStatusLabelKey,
  statusKindByReconcileStatus,
} from '../../integrations/model'
import {
  IntegrationKind,
  IntegrationMode,
  ReconcileStatus,
} from '../../proto/generated/integration_pb'
import { UrlTree } from '../../urls'

const MANAGER_ROLES = [ 'owner', 'admin' ]

const iconByKind = {
  [IntegrationKind.UNSPECIFIED]: LuPlug,
  [IntegrationKind.GIT]: LuGitBranch,
  [IntegrationKind.MAIL]: LuMail,
  [IntegrationKind.REGISTRY]: LuPackage,
  [IntegrationKind.PERFORCE]: LuContainer,
  [IntegrationKind.RUNNER]: LuHardHat,
} as const satisfies Record<IntegrationKind, typeof LuPlug>

// The integrations console (docs/integrations.md): the org's services, per
// project - managed containers and linked externals - with the reconciler
// state in the status grammar. Admins provision and manage; members see what
// they can reach. No realtime events yet: mutations refetch.
export function Integrations() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const navigate = useNavigate()
  const confirm = useConfirm()

  const orgs = useAppSelector((state) => state.sessionContext.orgs)
  const activeOrgId = useAppSelector((state) => state.sessionContext.activeOrgId)
  const activeOrg = orgs.find((org) => org.id === activeOrgId)
  const canManage = Boolean(activeOrg && MANAGER_ROLES.includes(activeOrg.role))

  const projects = useAppSelector((state) => state.projects.projects)
  const activeProjectId = useAppSelector((state) => state.projects.activeProjectId)

  const [ projectId, setProjectId ] = useState<string | null>(activeProjectId)
  const [ integrations, setIntegrations ] = useState<Integration[]>([])
  const [ drives, setDrives ] = useState<DriveView[]>([])
  const [ isLoading, setIsLoading ] = useState(true)
  const [ editTarget, setEditTarget ] = useState<Integration | undefined>(undefined)
  const [ isModalOpen, setIsModalOpen ] = useState(false)
  const [ noticeMessage, setNoticeMessage ] = useState<string | null>(null)

  const shownProjectId = projectId ?? activeProjectId

  async function loadIntegrations(targetProjectId: string) {
    setIsLoading(true)
    setNoticeMessage(null)
    try {
      const response = await listIntegrations(targetProjectId)
      setIntegrations(response.integrations)
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 403) {
        navigate(UrlTree.accessDenied)
        return
      }
      console.debug('integrations unavailable', { error })
      setNoticeMessage(t('auth:settings.integrations.loadFailed'))
    }
    finally {
      setIsLoading(false)
    }
  }

  useEffect(() => {
    if (shownProjectId) {
      loadIntegrations(shownProjectId)
    }
  }, [ shownProjectId ])

  // The binding pickers need the project's Drive roster.
  useEffect(() => {
    if (!shownProjectId || !canManage) {
      return
    }
    listDrives(shownProjectId)
      .then((response) => setDrives(response.drives.map(toDriveView)))
      .catch((error) => {
        console.debug('drive roster unavailable for binding pickers', { error })
      })
  }, [ shownProjectId, canManage ])

  // Wraps a row action with the shared 403/error handling and a refetch.
  async function run(action: () => Promise<unknown>) {
    setNoticeMessage(null)
    try {
      await action()
      if (shownProjectId) {
        await loadIntegrations(shownProjectId)
      }
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 403) {
        navigate(UrlTree.accessDenied)
        return
      }
      console.debug('integration action failed', { error })
      setNoticeMessage(t('auth:settings.integrations.updateFailed'))
    }
  }

  return <section className='w-full'>
    <div className='relaxed'>
      <h2 className='text-2xl'>
        <strong>{ t('auth:settings.integrations.title') }</strong>
      </h2>
      <p className='opacity-80'>{ t('auth:settings.integrations.subtitle') }</p>
    </div>

    <div className='relaxed level items-end'>
      <Select
        size='sm'
        className='max-w-xs'
        label={t('auth:settings.integrations.projectLabel')}
        selectedKeys={shownProjectId ? [ shownProjectId ] : []}
        onChange={(event) => {
          if (event.target.value) {
            setProjectId(event.target.value)
          }
        }}
      >
        { projects.map((project) =>
          <SelectItem key={project.id}>{ project.name }</SelectItem>,
        ) }
      </Select>
      { canManage
        ? <Button
          size='sm'
          color='primary'
          variant='flat'
          startContent={<LuPlus className='h-4 w-4' />}
          isDisabled={!shownProjectId}
          onPress={() => {
            setEditTarget(undefined)
            setIsModalOpen(true)
          }}
        >
          <span>{ t('auth:settings.integrations.createAction') }</span>
        </Button>
        : null }
    </div>

    { noticeMessage
      ? <p className='compact text-sm opacity-80'>{ noticeMessage }</p>
      : null }

    { isLoading
      ? <LoadingState label={t('auth:settings.integrations.loading')} />
      : null }

    { !isLoading && !integrations.length
      ? <EmptyState
        icon={<LuPlug />}
        title={t('auth:settings.integrations.empty')}
        description={canManage
          ? t('auth:settings.integrations.emptyHintAdmin')
          : t('auth:settings.integrations.emptyHint')}
      />
      : null }

    { !isLoading && integrations.map((integration) => {
      const KindIcon = iconByKind[integration.kind]
      const isManaged = integration.mode === IntegrationMode.MANAGED
      const rowClassName = 'compact level items-center rounded-lg border border-foreground/10 p-3'
      return <div key={integration.id?.value} className={rowClassName}>
        <div className='flex min-w-0 items-center gap-3'>
          <KindIcon className='h-5 w-5 shrink-0 opacity-60' />
          <div className='min-w-0'>
            <div className='flex items-center gap-2 truncate font-medium'>
              <span>{ integration.name }</span>
              <Chip size='sm' variant='flat'>{
                isManaged
                  ? t('auth:settings.integrations.modeManaged')
                  : t('auth:settings.integrations.modeLinked')
              }</Chip>
            </div>
            <div className='truncate text-xs opacity-60'>
              { t(`common:${integrationKindMeta[integration.kind].labelKey}`) }
              { integration.statusDetail
                ? ` · ${integration.statusDetail}`
                : '' }
            </div>
          </div>
        </div>
        <div className='flex shrink-0 items-center gap-3'>
          <StatusIndicator
            size='sm'
            kind={statusKindByReconcileStatus[integration.status]}
            label={t(`common:${reconcileStatusLabelKey[integration.status]}`)}
          />
          { canManage
            ? <Dropdown placement='bottom-end'>
              <DropdownTrigger>
                <Button
                  isIconOnly
                  size='sm'
                  variant='light'
                  aria-label={t('auth:settings.integrations.actionsLabel')}
                >
                  <LuEllipsisVertical className='h-4 w-4' />
                </Button>
              </DropdownTrigger>
              <DropdownMenu aria-label={t('auth:settings.integrations.actionsLabel')}>
                <DropdownItem
                  key='edit'
                  onPress={() => {
                    setEditTarget(integration)
                    setIsModalOpen(true)
                  }}
                >
                  { t('common:actions.edit') }
                </DropdownItem>
                <DropdownItem
                  key='start'
                  className={isManaged && integration.status !== ReconcileStatus.RUNNING ? '' : 'hidden'}
                  onPress={() => run(() => startIntegration(integration.id?.value ?? ''))}
                >
                  { t('auth:settings.integrations.startAction') }
                </DropdownItem>
                <DropdownItem
                  key='stop'
                  className={isManaged && integration.status === ReconcileStatus.RUNNING ? '' : 'hidden'}
                  onPress={() => run(() => stopIntegration(integration.id?.value ?? ''))}
                >
                  { t('auth:settings.integrations.stopAction') }
                </DropdownItem>
                <DropdownItem
                  key='delete'
                  color='danger'
                  onPress={() => confirm({
                    title: t('auth:settings.integrations.delete'),
                    message: t('auth:settings.integrations.deleteConfirm', { name: integration.name }),
                    confirmLabel: t('auth:settings.integrations.deleteAction'),
                    confirmColor: 'danger',
                    onConfirm: () => run(() => deleteIntegration(integration.id?.value ?? '')),
                  })}
                >
                  { t('auth:settings.integrations.delete') }
                </DropdownItem>
              </DropdownMenu>
            </Dropdown>
            : null }
        </div>
      </div>
    }) }

    { shownProjectId
      ? <IntegrationModal
        isOpen={isModalOpen}
        onClose={() => setIsModalOpen(false)}
        integration={editTarget}
        projectId={shownProjectId}
        drives={drives}
        onSaved={() => {
          loadIntegrations(shownProjectId)
        }}
      />
      : null }
  </section>
}
