// Copyright © 2026 Jalapeno Labs

import type { DriveConfig } from '../proto/generated/storage_pb'

// Lib
import { create } from '@bufbuild/protobuf'

// Misc
// The generated nested-message schema names carry underscores; alias them to
// camelCase locals to satisfy the house style.
import {
  DriveConfig_FsSchema as driveConfigFsSchema,
  DriveConfig_S3Schema as driveConfigS3Schema,
  DriveConfig_SftpSchema as driveConfigSftpSchema,
  DriveConfigSchema,
  DriveKind,
} from '../proto/generated/storage_pb'

// The Drive connection form model (docs/storage.md): plain serializable form
// values with one group per provider kind. All groups stay in state so
// switching kind while creating never loses typed values; only the active
// group is validated and serialized. The provider credential is NOT part of
// this model: it is a write-only string that rides the create/update request
// directly and is never echoed back.
export type DriveConfigFormValues = {
  kind: DriveKind
  fs: {
    root: string
  }
  s3: {
    endpoint: string
    region: string
    bucket: string
    prefix: string
    sse: boolean
  }
  sftp: {
    host: string
    port: string
    user: string
    basePath: string
  }
}

export function emptyDriveConfigValues(): DriveConfigFormValues {
  return {
    kind: DriveKind.FS,
    fs: { root: '' },
    s3: {
      endpoint: '',
      region: '',
      bucket: '',
      prefix: '',
      sse: false,
    },
    sftp: {
      host: '',
      port: '22',
      user: '',
      basePath: '',
    },
  }
}

// Seeds the form from an existing Drive's config (edit mode). Groups the
// config does not carry keep their empty defaults.
export function configValuesFromDrive(config: DriveConfig | undefined): DriveConfigFormValues {
  const values = emptyDriveConfigValues()
  if (!config?.kind.case) {
    console.debug('drive config has no active kind group, seeding an empty form')
    return values
  }

  if (config.kind.case === 'fs') {
    values.kind = DriveKind.FS
    values.fs = { root: config.kind.value.root }
  }
  else if (config.kind.case === 's3') {
    values.kind = DriveKind.S3
    values.s3 = {
      endpoint: config.kind.value.endpoint,
      region: config.kind.value.region,
      bucket: config.kind.value.bucket,
      prefix: config.kind.value.prefix,
      sse: config.kind.value.sse,
    }
  }
  else {
    values.kind = DriveKind.SFTP
    values.sftp = {
      host: config.kind.value.host,
      port: String(config.kind.value.port),
      user: config.kind.value.user,
      basePath: config.kind.value.basePath,
    }
  }
  return values
}

// Validates the ACTIVE group only. Returns i18n keys per field path (empty
// object = valid); the form translates at render.
export function validateDriveConfigValues(values: DriveConfigFormValues): Record<string, string> {
  const errors: Record<string, string> = {}

  if (values.kind === DriveKind.FS) {
    if (!values.fs.root.trim()) {
      errors['fs.root'] = 'drive.config.errors.rootRequired'
    }
    else if (!values.fs.root.trim().startsWith('/')) {
      errors['fs.root'] = 'drive.config.errors.rootAbsolute'
    }
  }
  else if (values.kind === DriveKind.S3) {
    if (!values.s3.bucket.trim()) {
      errors['s3.bucket'] = 'drive.config.errors.bucketRequired'
    }
    if (!values.s3.region.trim() && !values.s3.endpoint.trim()) {
      errors['s3.region'] = 'drive.config.errors.regionRequired'
    }
  }
  else if (values.kind === DriveKind.SFTP) {
    if (!values.sftp.host.trim()) {
      errors['sftp.host'] = 'drive.config.errors.hostRequired'
    }
    if (!values.sftp.user.trim()) {
      errors['sftp.user'] = 'drive.config.errors.userRequired'
    }
    const port = Number(values.sftp.port)
    if (!Number.isInteger(port) || port < 1 || port > 65535) {
      errors['sftp.port'] = 'drive.config.errors.portInvalid'
    }
  }
  else {
    errors['kind'] = 'drive.config.errors.kindRequired'
  }

  return errors
}

// Serializes the active group to the generated `DriveConfig` oneof. Call only
// after validation passes.
export function toDriveConfig(values: DriveConfigFormValues): DriveConfig {
  if (values.kind === DriveKind.S3) {
    return create(DriveConfigSchema, {
      kind: {
        case: 's3',
        value: create(driveConfigS3Schema, {
          endpoint: values.s3.endpoint.trim(),
          region: values.s3.region.trim(),
          bucket: values.s3.bucket.trim(),
          prefix: values.s3.prefix.trim(),
          sse: values.s3.sse,
        }),
      },
    })
  }

  if (values.kind === DriveKind.SFTP) {
    return create(DriveConfigSchema, {
      kind: {
        case: 'sftp',
        value: create(driveConfigSftpSchema, {
          host: values.sftp.host.trim(),
          port: Number(values.sftp.port),
          user: values.sftp.user.trim(),
          basePath: values.sftp.basePath.trim(),
        }),
      },
    })
  }

  return create(DriveConfigSchema, {
    kind: {
      case: 'fs',
      value: create(driveConfigFsSchema, { root: values.fs.root.trim() }),
    },
  })
}
