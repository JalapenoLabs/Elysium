// Copyright © 2026 Jalapeno Labs

import type { DriveView } from '../../drive/driveView'
import type { Integration } from '../../proto/generated/integration_pb'

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Lib
import { create } from '@bufbuild/protobuf'

// UI
import {
  Button,
  Input,
  Modal,
  ModalBody,
  ModalContent,
  ModalFooter,
  ModalHeader,
  Select,
  SelectItem,
  Textarea,
} from '@heroui/react'
import { DrivePicker } from '../../drive/DrivePicker'
import { SecretInput } from '../../components/form/SecretInput'

// Utility
import { parseJsonSafe } from '@jalapenolabs/uikit'

// Misc
import {
  createIntegration,
  setIntegrationBindings,
  updateIntegration,
} from '../../api/routes/integrationRoutes'
import { integrationKindMeta, storageRolesByKind } from '../../integrations/model'
import {
  IntegrationKind,
  IntegrationMode,
  IntegrationStorageBindingSchema,
  IntegrationVisibility,
  OwnerScope,
} from '../../proto/generated/integration_pb'

type Props = {
  isOpen: boolean
  onClose: () => void
  // Set for edit mode; absent for create.
  integration?: Integration
  projectId: string
  // The project's Drives, for the capability-eligible binding pickers.
  drives: DriveView[]
  onSaved: () => void
}

const selectableKinds = [
  IntegrationKind.GIT,
  IntegrationKind.MAIL,
  IntegrationKind.REGISTRY,
  IntegrationKind.PERFORCE,
  IntegrationKind.RUNNER,
] as const

// Create/edit an integration (docs/integrations.md): kind + mode + scope are
// immutable once created; name, visibility, config, credential, and the
// storage-role bindings stay editable. Bindings render one capability-eligible
// DrivePicker per role the kind requires.
export function IntegrationModal(props: Props) {
  const { t } = useTranslation([ 'auth', 'common' ])

  const [ name, setName ] = useState('')
  const [ kind, setKind ] = useState<IntegrationKind>(IntegrationKind.GIT)
  const [ mode, setMode ] = useState<IntegrationMode>(IntegrationMode.MANAGED)
  const [ visibility, setVisibility ] = useState(IntegrationVisibility.ORG_WIDE)
  const [ configJson, setConfigJson ] = useState('')
  const [ credential, setCredential ] = useState('')
  const [ driveIdByRole, setDriveIdByRole ] = useState<Record<string, string>>({})
  const [ isBusy, setIsBusy ] = useState(false)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)

  const isEditing = Boolean(props.integration)

  useEffect(() => {
    if (!props.isOpen) {
      return
    }
    setName(props.integration?.name ?? '')
    setKind(props.integration?.kind ?? IntegrationKind.GIT)
    setMode(props.integration?.mode ?? IntegrationMode.MANAGED)
    setVisibility(props.integration?.visibility ?? IntegrationVisibility.ORG_WIDE)
    setConfigJson(props.integration?.configJson ?? '')
    setCredential('')
    setDriveIdByRole(Object.fromEntries(
      (props.integration?.bindings ?? []).map((binding) => [
        binding.role,
        binding.driveId?.value ?? '',
      ]),
    ))
    setErrorMessage(null)
  }, [ props.isOpen, props.integration ])

  const roles = storageRolesByKind[kind]
  const isConfigValid = !configJson.trim() || parseJsonSafe(configJson) !== null
  const canSave = Boolean(name.trim()) && isConfigValid && !isBusy

  async function onSave() {
    setIsBusy(true)
    setErrorMessage(null)
    try {
      let saved: Integration
      if (props.integration) {
        saved = await updateIntegration(props.integration.id?.value ?? '', {
          name: name.trim(),
          visibility,
          configJson: configJson.trim(),
          credential,
        })
      }
      else {
        saved = await createIntegration({
          projectId: props.projectId,
          kind,
          mode,
          // The console manages project infrastructure; personal linking gets
          // its own flow with the #36 policy work.
          ownerScope: OwnerScope.PROJECT,
          name: name.trim(),
          visibility,
          configJson: configJson.trim(),
          credential,
        })
      }

      const bindings = roles
        .filter((role) => driveIdByRole[role.role])
        .map((role) => create(IntegrationStorageBindingSchema, {
          role: role.role,
          driveId: { value: driveIdByRole[role.role] },
          prefix: '',
        }))
      if (mode === IntegrationMode.MANAGED && bindings.length) {
        await setIntegrationBindings(saved.id?.value ?? '', bindings)
      }

      props.onSaved()
      props.onClose()
    }
    catch (error) {
      console.debug('integration save failed', { error })
      setErrorMessage(t('auth:settings.integrations.saveFailed'))
    }
    finally {
      setIsBusy(false)
    }
  }

  return <Modal isOpen={props.isOpen} onClose={props.onClose} size='2xl' scrollBehavior='inside'>
    <ModalContent>
      <ModalHeader>{
        isEditing
          ? t('auth:settings.integrations.editTitle', { name: props.integration?.name })
          : t('auth:settings.integrations.createTitle')
      }</ModalHeader>
      <ModalBody>
        <div className='compact level items-start'>
          <Select
            label={t('auth:settings.integrations.kindLabel')}
            className='w-full'
            isDisabled={isBusy || isEditing}
            selectedKeys={[ String(kind) ]}
            onChange={(event) => {
              if (event.target.value) {
                setKind(Number(event.target.value) as IntegrationKind)
              }
            }}
          >
            { selectableKinds.map((selectableKind) =>
              <SelectItem key={String(selectableKind)}>
                { t(`common:${integrationKindMeta[selectableKind].labelKey}`) }
              </SelectItem>,
            ) }
          </Select>
          <Select
            label={t('auth:settings.integrations.modeLabel')}
            className='w-full'
            isDisabled={isBusy || isEditing}
            selectedKeys={[ String(mode) ]}
            onChange={(event) => {
              if (event.target.value) {
                setMode(Number(event.target.value) as IntegrationMode)
              }
            }}
          >
            <SelectItem key={String(IntegrationMode.MANAGED)}>
              { t('auth:settings.integrations.modeManaged') }
            </SelectItem>
            <SelectItem key={String(IntegrationMode.LINKED)}>
              { t('auth:settings.integrations.modeLinked') }
            </SelectItem>
          </Select>
        </div>
        <div className='compact'>
          <Input
            autoFocus
            label={t('auth:settings.integrations.nameLabel')}
            value={name}
            isDisabled={isBusy}
            onValueChange={setName}
          />
        </div>
        <div className='compact'>
          <Select
            label={t('auth:settings.integrations.visibilityLabel')}
            isDisabled={isBusy}
            selectedKeys={[ String(visibility) ]}
            onChange={(event) => {
              if (event.target.value) {
                setVisibility(Number(event.target.value) as IntegrationVisibility)
              }
            }}
          >
            <SelectItem key={String(IntegrationVisibility.ORG_WIDE)}>
              { t('auth:settings.integrations.visibilityOrgWide') }
            </SelectItem>
            <SelectItem key={String(IntegrationVisibility.RESTRICTED)}>
              { t('auth:settings.integrations.visibilityRestricted') }
            </SelectItem>
          </Select>
        </div>

        {/* Storage-role bindings, managed mode only: one capability-eligible
            Drive per role the kind requires. */}
        { mode === IntegrationMode.MANAGED && Boolean(roles.length) && (
          <div className='compact'>
            <p className='compact text-sm font-medium opacity-70'>{
              t('auth:settings.integrations.bindingsTitle')
            }</p>
            { roles.map((role) =>
              <div key={role.role} className='compact'>
                <DrivePicker
                  drives={props.drives}
                  requiredCapability={role.capability}
                  selectedDriveId={driveIdByRole[role.role] ?? null}
                  label={t(`common:${role.labelKey}`)}
                  isDisabled={isBusy}
                  onSelect={(driveId) => {
                    setDriveIdByRole((previous) => ({ ...previous, [role.role]: driveId }))
                  }}
                />
              </div>,
            ) }
          </div>
        ) }

        <div className='compact'>
          <Textarea
            label={t('auth:settings.integrations.configLabel')}
            placeholder='{ }'
            minRows={3}
            isDisabled={isBusy}
            isInvalid={!isConfigValid}
            errorMessage={isConfigValid ? undefined : t('auth:settings.integrations.configInvalid')}
            value={configJson}
            onValueChange={setConfigJson}
          />
        </div>
        <div className='compact'>
          <SecretInput
            label={t('auth:settings.integrations.credentialLabel')}
            value={credential}
            onValueChange={setCredential}
            isDisabled={isBusy}
          />
          { isEditing && (
            <p className='mt-1 text-xs opacity-50'>{
              t('auth:settings.integrations.credentialKeepHint')
            }</p>
          ) }
        </div>

        { errorMessage && (
          <p className='text-sm text-danger'>{ errorMessage }</p>
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
              : t('auth:settings.integrations.createAction')
          }</span>
        </Button>
      </ModalFooter>
    </ModalContent>
  </Modal>
}
