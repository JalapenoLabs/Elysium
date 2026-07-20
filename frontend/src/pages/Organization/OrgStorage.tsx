// Copyright © 2026 Jalapeno Labs

import type { Drive } from '../../proto/generated/storage_pb'
import type { OrgMember } from '../../api/routes/orgRoutes'
import type { Team } from '../../api/routes/teamRoutes'

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
import { LuCloud, LuEllipsisVertical, LuHardDrive, LuPlus, LuServer } from 'react-icons/lu'
import { DriveModal } from './DriveModal'
import { EmptyState } from '../../components/states/EmptyState'
import { LoadingState } from '../../components/states/LoadingState'
import { StatusIndicator } from '../../components/status/StatusIndicator'

// Utility
import { HTTPError } from 'ky'

// Misc
import { useConfirm } from '../../hooks/useConfirm'
import { deleteDrive, listDrives, setDefaultDrive, testDrive } from '../../api/routes/driveRoutes'
import { listOrgMembers } from '../../api/routes/orgRoutes'
import { listTeams } from '../../api/routes/teamRoutes'
import { capabilityLabelKeyByCapability, driveKindMeta } from '../../drive/driveView'
import { DriveKind, DriveVisibility } from '../../proto/generated/storage_pb'
import { UrlTree } from '../../urls'

const MANAGER_ROLES = [ 'owner', 'admin' ]

const iconByKind = {
  [DriveKind.UNSPECIFIED]: LuHardDrive,
  [DriveKind.FS]: LuHardDrive,
  [DriveKind.S3]: LuCloud,
  [DriveKind.SFTP]: LuServer,
} as const satisfies Record<DriveKind, typeof LuHardDrive>

// The org's Drives, per project (docs/storage.md): list, create, edit, test,
// set default, delete. Members see the Drives they can reach; owners/admins
// manage. The file browser panel consumes these Drives from the workspace.
export function OrgStorage() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const navigate = useNavigate()
  const confirm = useConfirm()

  const orgs = useAppSelector((state) => state.sessionContext.orgs)
  const activeOrgId = useAppSelector((state) => state.sessionContext.activeOrgId)
  const activeOrg = orgs.find((org) => org.id === activeOrgId)
  const canManage = Boolean(activeOrg && MANAGER_ROLES.includes(activeOrg.role))

  const projects = useAppSelector((state) => state.projects.projects)
  const activeProjectId = useAppSelector((state) => state.projects.activeProjectId)

  // Which project's Drives the page shows; starts on the active project.
  const [ projectId, setProjectId ] = useState<string | null>(activeProjectId)
  const [ drives, setDrives ] = useState<Drive[]>([])
  const [ teams, setTeams ] = useState<Team[]>([])
  const [ orgMembers, setOrgMembers ] = useState<OrgMember[]>([])
  const [ isLoading, setIsLoading ] = useState(true)
  const [ editTarget, setEditTarget ] = useState<Drive | undefined>(undefined)
  const [ isModalOpen, setIsModalOpen ] = useState(false)
  const [ noticeMessage, setNoticeMessage ] = useState<string | null>(null)

  const shownProjectId = projectId ?? activeProjectId

  async function loadDrives(targetProjectId: string) {
    setIsLoading(true)
    setNoticeMessage(null)
    try {
      const response = await listDrives(targetProjectId)
      setDrives(response.drives)
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 403) {
        navigate(UrlTree.accessDenied)
        return
      }
      console.debug('drives unavailable', { error })
      setNoticeMessage(t('auth:settings.storage.loadFailed'))
    }
    finally {
      setIsLoading(false)
    }
  }

  useEffect(() => {
    if (shownProjectId) {
      loadDrives(shownProjectId)
    }
  }, [ shownProjectId ])

  // The grant pickers need the org rosters; load once.
  useEffect(() => {
    if (!activeOrgId || !canManage) {
      return
    }
    Promise.all([ listTeams(activeOrgId), listOrgMembers(activeOrgId) ])
      .then(([ teamsResponse, membersResponse ]) => {
        setTeams(teamsResponse.teams)
        setOrgMembers(membersResponse.members)
      })
      .catch((error) => {
        console.debug('rosters unavailable for the grant pickers', { error })
      })
  }, [ activeOrgId, canManage ])

  // Wraps a row action with the shared 403/error handling and a reload.
  async function run(action: () => Promise<unknown>) {
    setNoticeMessage(null)
    try {
      await action()
      if (shownProjectId) {
        await loadDrives(shownProjectId)
      }
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 403) {
        navigate(UrlTree.accessDenied)
        return
      }
      console.debug('drive action failed', { error })
      setNoticeMessage(t('auth:settings.storage.updateFailed'))
    }
  }

  async function onTest(drive: Drive) {
    setNoticeMessage(null)
    try {
      const test = await testDrive(drive.id?.value ?? '')
      if (test.reachable) {
        setNoticeMessage(t('auth:settings.storage.testPassed', { name: drive.name }))
      }
      else {
        setNoticeMessage(t('auth:settings.storage.testFailed', { message: test.message }))
      }
    }
    catch (error) {
      console.debug('drive test errored unexpectedly', { error })
      setNoticeMessage(t('auth:settings.storage.updateFailed'))
    }
  }

  return <section className='w-full'>
    <div className='relaxed'>
      <h2 className='text-2xl'>
        <strong>{ t('auth:settings.storage.title') }</strong>
      </h2>
      <p className='opacity-80'>{ t('auth:settings.storage.subtitle') }</p>
    </div>

    <div className='relaxed level items-end'>
      <Select
        size='sm'
        className='max-w-xs'
        label={t('auth:settings.storage.projectLabel')}
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
          <span>{ t('auth:settings.storage.createAction') }</span>
        </Button>
        : null }
    </div>

    { noticeMessage
      ? <p className='compact text-sm opacity-80'>{ noticeMessage }</p>
      : null }

    { isLoading
      ? <LoadingState label={t('auth:settings.storage.loading')} />
      : null }

    { !isLoading && !drives.length
      ? <EmptyState
        icon={<LuHardDrive />}
        title={t('auth:settings.storage.empty')}
        description={canManage
          ? t('auth:settings.storage.emptyHintAdmin')
          : t('auth:settings.storage.emptyHint')}
      />
      : null }

    { !isLoading && drives.map((drive) => {
      const KindIcon = iconByKind[drive.kind]
      const rowClassName = 'compact level items-center rounded-lg border border-foreground/10 p-3'
      return <div key={drive.id?.value} className={rowClassName}>
        <div className='flex min-w-0 items-center gap-3'>
          <KindIcon className='h-5 w-5 shrink-0 opacity-60' />
          <div className='min-w-0'>
            <div className='flex items-center gap-2 truncate font-medium'>
              <span>{ drive.name }</span>
              { drive.isDefault
                ? <Chip size='sm' variant='flat' color='primary'>{
                  t('auth:settings.storage.defaultBadge')
                }</Chip>
                : null }
              { drive.visibility === DriveVisibility.RESTRICTED
                ? <Chip size='sm' variant='flat'>{
                  t('auth:settings.storage.restrictedBadge')
                }</Chip>
                : null }
            </div>
            <p className='truncate text-xs opacity-60'>
              { t(`common:${driveKindMeta[drive.kind].labelKey}`) }
              { ' · ' }
              { drive.capabilities
                .map((capability) => t(`common:${capabilityLabelKeyByCapability[capability]}`))
                .join(', ') }
            </p>
          </div>
        </div>
        <div className='flex shrink-0 items-center gap-3'>
          <StatusIndicator
            size='sm'
            kind={drive.credentialSet || drive.kind === DriveKind.FS ? 'done' : 'needsInput'}
            label={drive.credentialSet || drive.kind === DriveKind.FS
              ? t('auth:settings.storage.credentialReady')
              : t('auth:settings.storage.credentialMissing')}
          />
          { canManage
            ? <Dropdown placement='bottom-end'>
              <DropdownTrigger>
                <Button isIconOnly size='sm' variant='light' aria-label={t('auth:settings.storage.actionsLabel')}>
                  <LuEllipsisVertical className='h-4 w-4' />
                </Button>
              </DropdownTrigger>
              <DropdownMenu aria-label={t('auth:settings.storage.actionsLabel')}>
                <DropdownItem
                  key='edit'
                  onPress={() => {
                    setEditTarget(drive)
                    setIsModalOpen(true)
                  }}
                >
                  { t('common:actions.edit') }
                </DropdownItem>
                <DropdownItem key='test' onPress={() => onTest(drive)}>
                  { t('auth:settings.storage.testAction') }
                </DropdownItem>
                <DropdownItem
                  key='default'
                  className={drive.isDefault ? 'hidden' : ''}
                  onPress={() => run(() => setDefaultDrive(drive.id?.value ?? ''))}
                >
                  { t('auth:settings.storage.setDefaultAction') }
                </DropdownItem>
                <DropdownItem
                  key='delete'
                  color='danger'
                  onPress={() => confirm({
                    title: t('auth:settings.storage.delete'),
                    message: t('auth:settings.storage.deleteConfirm', { name: drive.name }),
                    confirmLabel: t('auth:settings.storage.deleteAction'),
                    confirmColor: 'danger',
                    onConfirm: () => run(() => deleteDrive(drive.id?.value ?? '')),
                  })}
                >
                  { t('auth:settings.storage.delete') }
                </DropdownItem>
              </DropdownMenu>
            </Dropdown>
            : null }
        </div>
      </div>
    }) }

    { shownProjectId
      ? <DriveModal
        isOpen={isModalOpen}
        onClose={() => setIsModalOpen(false)}
        drive={editTarget}
        projectId={shownProjectId}
        teams={teams}
        orgMembers={orgMembers}
        onSaved={() => {
          loadDrives(shownProjectId)
        }}
      />
      : null }
  </section>
}
