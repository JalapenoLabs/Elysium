// Copyright © 2026 Jalapeno Labs

import type { DriveConfigFormValues } from './driveConfigForm'

// Core
import { useTranslation } from 'react-i18next'

// UI
import { Input, Select, SelectItem, Switch } from '@heroui/react'
import { SecretInput } from '../components/form/SecretInput'

// Misc
import { DriveKind } from '../proto/generated/storage_pb'
import { driveKindMeta } from './driveView'

// The per-kind Drive connection fields (docs/storage.md), for the create/edit
// surfaces #25 builds once the endpoints land. The caller owns the values (see
// driveConfigForm.ts) and the credential separately: the credential is
// write-only, so edit mode shows an empty field that means "keep the sealed
// secret" rather than ever echoing one back.
type Props = {
  values: DriveConfigFormValues
  onChange: (values: DriveConfigFormValues) => void
  // i18n keys per field path from validateDriveConfigValues; translated here.
  errors: Record<string, string>
  credential: string
  onCredentialChange: (credential: string) => void
  // Edit mode: the kind is immutable and the credential placeholder explains
  // the keep-existing semantics.
  isEditing?: boolean
  isDisabled?: boolean
}

const selectableKinds = [ DriveKind.FS, DriveKind.S3, DriveKind.SFTP ] as const

export function DriveConfigFields(props: Props) {
  const { t } = useTranslation()

  function fieldError(path: string): string | undefined {
    const key = props.errors[path]
    if (key) {
      return t(key)
    }
    return undefined
  }

  return <div>
    <div className='compact'>
      <Select
        label={t('drive.config.kindLabel')}
        isDisabled={props.isDisabled || props.isEditing}
        selectedKeys={[ String(props.values.kind) ]}
        onChange={(event) => {
          if (event.target.value) {
            props.onChange({ ...props.values, kind: Number(event.target.value) as DriveKind })
          }
        }}
      >
        { selectableKinds.map((kind) =>
          <SelectItem key={String(kind)}>
            { t(driveKindMeta[kind].labelKey) }
          </SelectItem>,
        ) }
      </Select>
    </div>

    { props.values.kind === DriveKind.FS && (
      <div className='compact'>
        <Input
          label={t('drive.config.fs.rootLabel')}
          placeholder='/srv/elysium/drive'
          isDisabled={props.isDisabled}
          isInvalid={Boolean(fieldError('fs.root'))}
          errorMessage={fieldError('fs.root')}
          value={props.values.fs.root}
          onValueChange={(root) => props.onChange({ ...props.values, fs: { root }})}
        />
      </div>
    ) }

    { props.values.kind === DriveKind.S3 && (
      <>
        <div className='compact level items-start'>
          <Input
            label={t('drive.config.s3.bucketLabel')}
            className='w-full'
            isDisabled={props.isDisabled}
            isInvalid={Boolean(fieldError('s3.bucket'))}
            errorMessage={fieldError('s3.bucket')}
            value={props.values.s3.bucket}
            onValueChange={(bucket) => {
              props.onChange({ ...props.values, s3: { ...props.values.s3, bucket }})
            }}
          />
          <Input
            label={t('drive.config.s3.regionLabel')}
            placeholder='us-east-1'
            className='w-full'
            isDisabled={props.isDisabled}
            isInvalid={Boolean(fieldError('s3.region'))}
            errorMessage={fieldError('s3.region')}
            value={props.values.s3.region}
            onValueChange={(region) => {
              props.onChange({ ...props.values, s3: { ...props.values.s3, region }})
            }}
          />
        </div>
        <div className='compact level items-start'>
          <Input
            label={t('drive.config.s3.endpointLabel')}
            placeholder='https://s3.example.com'
            className='w-full'
            isDisabled={props.isDisabled}
            value={props.values.s3.endpoint}
            onValueChange={(endpoint) => {
              props.onChange({ ...props.values, s3: { ...props.values.s3, endpoint }})
            }}
          />
          <Input
            label={t('drive.config.s3.prefixLabel')}
            className='w-full'
            isDisabled={props.isDisabled}
            value={props.values.s3.prefix}
            onValueChange={(prefix) => {
              props.onChange({ ...props.values, s3: { ...props.values.s3, prefix }})
            }}
          />
        </div>
        <div className='compact'>
          <Switch
            size='sm'
            isDisabled={props.isDisabled}
            isSelected={props.values.s3.sse}
            onValueChange={(sse) => {
              props.onChange({ ...props.values, s3: { ...props.values.s3, sse }})
            }}
          >
            <span className='text-sm'>{ t('drive.config.s3.sseLabel') }</span>
          </Switch>
        </div>
      </>
    ) }

    { props.values.kind === DriveKind.SFTP && (
      <>
        <div className='compact level items-start'>
          <Input
            label={t('drive.config.sftp.hostLabel')}
            className='w-full'
            isDisabled={props.isDisabled}
            isInvalid={Boolean(fieldError('sftp.host'))}
            errorMessage={fieldError('sftp.host')}
            value={props.values.sftp.host}
            onValueChange={(host) => {
              props.onChange({ ...props.values, sftp: { ...props.values.sftp, host }})
            }}
          />
          <Input
            label={t('drive.config.sftp.portLabel')}
            className='w-28'
            isDisabled={props.isDisabled}
            isInvalid={Boolean(fieldError('sftp.port'))}
            errorMessage={fieldError('sftp.port')}
            value={props.values.sftp.port}
            onValueChange={(port) => {
              props.onChange({ ...props.values, sftp: { ...props.values.sftp, port }})
            }}
          />
        </div>
        <div className='compact level items-start'>
          <Input
            label={t('drive.config.sftp.userLabel')}
            className='w-full'
            isDisabled={props.isDisabled}
            isInvalid={Boolean(fieldError('sftp.user'))}
            errorMessage={fieldError('sftp.user')}
            value={props.values.sftp.user}
            onValueChange={(user) => {
              props.onChange({ ...props.values, sftp: { ...props.values.sftp, user }})
            }}
          />
          <Input
            label={t('drive.config.sftp.basePathLabel')}
            placeholder='/home/elysium'
            className='w-full'
            isDisabled={props.isDisabled}
            value={props.values.sftp.basePath}
            onValueChange={(basePath) => {
              props.onChange({ ...props.values, sftp: { ...props.values.sftp, basePath }})
            }}
          />
        </div>
      </>
    ) }

    { props.values.kind !== DriveKind.FS && (
      <div className='compact'>
        <SecretInput
          label={t('drive.config.credentialLabel')}
          value={props.credential}
          onValueChange={props.onCredentialChange}
          isDisabled={props.isDisabled}
        />
        { props.isEditing && (
          <p className='mt-1 text-xs opacity-50'>{
            t('drive.config.credentialKeepHint')
          }</p>
        ) }
      </div>
    ) }
  </div>
}
