// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'
import { create } from '@bufbuild/protobuf'

// Misc
import {
  configValuesFromDrive,
  emptyDriveConfigValues,
  toDriveConfig,
  validateDriveConfigValues,
} from './driveConfigForm'
import {
  DriveConfig_SftpSchema as driveConfigSftpSchema,
  DriveConfigSchema,
  DriveKind,
} from '../proto/generated/storage_pb'

describe('validateDriveConfigValues', () => {
  it('requires an absolute root for a filesystem drive', () => {
    const values = emptyDriveConfigValues()
    expect(validateDriveConfigValues(values)).toHaveProperty('fs.root')

    values.fs.root = 'relative/path'
    expect(validateDriveConfigValues(values)['fs.root']).toBe('drive.config.errors.rootAbsolute')

    values.fs.root = '/srv/drive'
    expect(validateDriveConfigValues(values)).toEqual({})
  })

  it('requires a bucket and either region or endpoint for s3', () => {
    const values = emptyDriveConfigValues()
    values.kind = DriveKind.S3

    let errors = validateDriveConfigValues(values)
    expect(errors).toHaveProperty('s3.bucket')
    expect(errors).toHaveProperty('s3.region')

    values.s3.bucket = 'elysium'
    values.s3.endpoint = 'https://minio.local'
    errors = validateDriveConfigValues(values)
    expect(errors).toEqual({})
  })

  it('validates the sftp group including the port range', () => {
    const values = emptyDriveConfigValues()
    values.kind = DriveKind.SFTP
    values.sftp = { host: 'files.example.com', port: '99999', user: 'elysium', basePath: '' }

    expect(validateDriveConfigValues(values)['sftp.port']).toBe('drive.config.errors.portInvalid')

    values.sftp.port = '22'
    expect(validateDriveConfigValues(values)).toEqual({})
  })

  it('only validates the active group', () => {
    const values = emptyDriveConfigValues()
    values.kind = DriveKind.S3
    values.s3.bucket = 'elysium'
    values.s3.region = 'us-east-1'
    // The untouched fs group stays empty and must not produce errors.
    expect(validateDriveConfigValues(values)).toEqual({})
  })
})

describe('toDriveConfig', () => {
  it('serializes the active group to the matching oneof case', () => {
    const values = emptyDriveConfigValues()
    values.kind = DriveKind.S3
    values.s3 = {
      endpoint: ' https://minio.local ',
      region: 'us-east-1',
      bucket: 'elysium',
      prefix: 'team/',
      sse: true,
    }

    const config = toDriveConfig(values)
    expect(config.kind.case).toBe('s3')
    if (config.kind.case === 's3') {
      expect(config.kind.value.endpoint).toBe('https://minio.local')
      expect(config.kind.value.sse).toBe(true)
    }
  })

  it('round-trips through configValuesFromDrive', () => {
    const config = create(DriveConfigSchema, {
      kind: {
        case: 'sftp',
        value: create(driveConfigSftpSchema, {
          host: 'files.example.com',
          port: 2222,
          user: 'elysium',
          basePath: '/srv',
        }),
      },
    })

    const values = configValuesFromDrive(config)
    expect(values.kind).toBe(DriveKind.SFTP)
    expect(values.sftp).toEqual({
      host: 'files.example.com',
      port: '2222',
      user: 'elysium',
      basePath: '/srv',
    })

    const roundTripped = toDriveConfig(values)
    expect(roundTripped).toEqual(config)
  })

  it('seeds an empty form when no config group is set', () => {
    const values = configValuesFromDrive(undefined)
    expect(values).toEqual(emptyDriveConfigValues())
  })
})
