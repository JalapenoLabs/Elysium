// Copyright © 2026 Jalapeno Labs

import type { Drive, DriveAccess } from '../../proto/generated/storage_pb'
import type { OrgMember } from '../../api/routes/orgRoutes'
import type { Team } from '../../api/routes/teamRoutes'

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Lib
import { create } from '@bufbuild/protobuf'

// UI
import {
  Button,
  Chip,
  Input,
  Modal,
  ModalBody,
  ModalContent,
  ModalFooter,
  ModalHeader,
  Select,
  SelectItem,
} from '@heroui/react'
import { DriveConfigFields } from '../../drive/DriveConfigFields'

// Utility
import { HTTPError } from 'ky'

// Misc
import {
  configValuesFromDrive,
  emptyDriveConfigValues,
  toDriveConfig,
  validateDriveConfigValues,
} from '../../drive/driveConfigForm'
import { createDrive, setDriveAccess, testDrive, updateDrive } from '../../api/routes/driveRoutes'
// The generated nested-enum name carries an underscore; alias it to camelCase
// for the house style.
import {
  DriveAccess_Principal as driveAccessPrincipal,
  DriveAccessSchema,
  DriveVisibility,
} from '../../proto/generated/storage_pb'

type Props = {
  isOpen: boolean
  onClose: () => void
  // Set for edit mode; absent for create.
  drive?: Drive
  projectId: string
  // Rosters for the restricted-visibility grant pickers.
  teams: Team[]
  orgMembers: OrgMember[]
  onSaved: () => void
}

// Create/edit a Drive (docs/storage.md): name, visibility, per-kind connection
// settings, and restricted-visibility grants. The connection test runs against
// the SAVED drive (the endpoint needs an id), so create saves first and then
// tests, reporting reachability either way. There is no grants read endpoint
// yet, so edit mode composes a replacement set from scratch.
export function DriveModal(props: Props) {
  const { t } = useTranslation([ 'auth', 'common' ])

  const [ name, setName ] = useState('')
  const [ visibility, setVisibility ] = useState(DriveVisibility.ORG_WIDE)
  const [ configValues, setConfigValues ] = useState(emptyDriveConfigValues())
  const [ credential, setCredential ] = useState('')
  const [ grantTeamIds, setGrantTeamIds ] = useState<string[]>([])
  const [ grantUserIds, setGrantUserIds ] = useState<string[]>([])
  const [ isBusy, setIsBusy ] = useState(false)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)
  const [ testResult, setTestResult ] = useState<string | null>(null)

  const isEditing = Boolean(props.drive)

  // Re-seed whenever the modal opens for a different target.
  useEffect(() => {
    if (!props.isOpen) {
      return
    }
    setName(props.drive?.name ?? '')
    setVisibility(props.drive?.visibility ?? DriveVisibility.ORG_WIDE)
    setConfigValues(props.drive
      ? configValuesFromDrive(props.drive.config)
      : emptyDriveConfigValues())
    setCredential('')
    setGrantTeamIds([])
    setGrantUserIds([])
    setErrorMessage(null)
    setTestResult(null)
  }, [ props.isOpen, props.drive ])

  const configErrors = validateDriveConfigValues(configValues)
  const canSave = Boolean(name.trim()) && !Object.keys(configErrors).length && !isBusy

  function buildGrants(): DriveAccess[] {
    const teamGrants = grantTeamIds.map((teamId) => create(DriveAccessSchema, {
      principal: driveAccessPrincipal.TEAM,
      principalId: { value: teamId },
    }))
    const userGrants = grantUserIds.map((userId) => create(DriveAccessSchema, {
      principal: driveAccessPrincipal.USER,
      principalId: { value: userId },
    }))
    return [ ...teamGrants, ...userGrants ]
  }

  async function onSave() {
    setIsBusy(true)
    setErrorMessage(null)
    setTestResult(null)
    try {
      const config = toDriveConfig(configValues)
      let saved: Drive
      if (props.drive) {
        saved = await updateDrive(props.drive.id?.value ?? '', {
          name: name.trim(),
          visibility,
          config,
          credential,
        })
      }
      else {
        saved = await createDrive({
          projectId: props.projectId,
          name: name.trim(),
          kind: configValues.kind,
          visibility,
          config,
          credential,
        })
      }

      if (visibility === DriveVisibility.RESTRICTED && (grantTeamIds.length || grantUserIds.length)) {
        await setDriveAccess(saved.id?.value ?? '', buildGrants())
      }

      // The test endpoint needs a saved Drive, so reachability reports after
      // save; a failure leaves the Drive saved and the message visible.
      const test = await testDrive(saved.id?.value ?? '')
      if (!test.reachable) {
        setTestResult(t('auth:settings.storage.testFailed', { message: test.message }))
        setIsBusy(false)
        props.onSaved()
        return
      }

      props.onSaved()
      props.onClose()
    }
    catch (error) {
      console.debug('drive save failed', { error })
      if (error instanceof HTTPError && error.response.status === 400) {
        setErrorMessage(t('auth:settings.storage.invalidConfig'))
      }
      else {
        setErrorMessage(t('auth:settings.storage.saveFailed'))
      }
    }
    finally {
      setIsBusy(false)
    }
  }

  return <Modal isOpen={props.isOpen} onClose={props.onClose} size='2xl' scrollBehavior='inside'>
    <ModalContent>
      <ModalHeader>{
        isEditing
          ? t('auth:settings.storage.editTitle', { name: props.drive?.name })
          : t('auth:settings.storage.createTitle')
      }</ModalHeader>
      <ModalBody>
        <div className='compact'>
          <Input
            autoFocus
            label={t('auth:settings.storage.nameLabel')}
            value={name}
            isDisabled={isBusy}
            onValueChange={setName}
          />
        </div>
        <div className='compact'>
          <Select
            label={t('auth:settings.storage.visibilityLabel')}
            isDisabled={isBusy}
            selectedKeys={[ String(visibility) ]}
            onChange={(event) => {
              if (event.target.value) {
                setVisibility(Number(event.target.value) as DriveVisibility)
              }
            }}
          >
            <SelectItem key={String(DriveVisibility.ORG_WIDE)}>
              { t('auth:settings.storage.visibilityOrgWide') }
            </SelectItem>
            <SelectItem key={String(DriveVisibility.RESTRICTED)}>
              { t('auth:settings.storage.visibilityRestricted') }
            </SelectItem>
          </Select>
        </div>

        { visibility === DriveVisibility.RESTRICTED && (
          <div className='compact'>
            <p className='compact text-sm opacity-70'>{
              isEditing
                ? t('auth:settings.storage.grantsReplaceHint')
                : t('auth:settings.storage.grantsHint')
            }</p>
            <div className='level items-start'>
              <Select
                size='sm'
                selectionMode='multiple'
                className='w-full'
                label={t('auth:settings.storage.grantTeamsLabel')}
                isDisabled={isBusy}
                selectedKeys={grantTeamIds}
                onSelectionChange={(keys) => setGrantTeamIds([ ...keys ].map(String))}
              >
                { props.teams.map((team) =>
                  <SelectItem key={team.id}>{ team.name }</SelectItem>,
                ) }
              </Select>
              <Select
                size='sm'
                selectionMode='multiple'
                className='w-full'
                label={t('auth:settings.storage.grantUsersLabel')}
                isDisabled={isBusy}
                selectedKeys={grantUserIds}
                onSelectionChange={(keys) => setGrantUserIds([ ...keys ].map(String))}
              >
                { props.orgMembers.map((member) =>
                  <SelectItem key={member.user_id}>
                    { member.display_name || member.email }
                  </SelectItem>,
                ) }
              </Select>
            </div>
          </div>
        ) }

        <DriveConfigFields
          values={configValues}
          onChange={setConfigValues}
          errors={configErrors}
          credential={credential}
          onCredentialChange={setCredential}
          isEditing={isEditing}
          isDisabled={isBusy}
        />

        { errorMessage && (
          <p className='text-sm text-danger'>{ errorMessage }</p>
        ) }
        { testResult && (
          <Chip color='warning' variant='flat' className='max-w-full'>
            <span className='truncate'>{ testResult }</span>
          </Chip>
        ) }
      </ModalBody>
      <ModalFooter>
        <Button variant='light' isDisabled={isBusy} onPress={props.onClose}>
          <span>{ t('common:actions.cancel') }</span>
        </Button>
        <Button color='primary' isDisabled={!canSave} isLoading={isBusy} onPress={onSave}>
          <span>{
            isEditing
              ? t('common:actions.save')
              : t('auth:settings.storage.createAction')
          }</span>
        </Button>
      </ModalFooter>
    </ModalContent>
  </Modal>
}
