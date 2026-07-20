// Copyright © 2026 Jalapeno Labs

import type { DriveView } from './driveView'
import type { StorageCapability } from '../proto/generated/storage_pb'

// Core
import { useTranslation } from 'react-i18next'

// UI
import { Select, SelectItem } from '@heroui/react'
import { LuCloud, LuFolder, LuHardDrive, LuServer } from 'react-icons/lu'

// Misc
import { DriveKind } from '../proto/generated/storage_pb'
import {
  capabilityLabelKeyByCapability,
  driveKindMeta,
  splitDrivesByCapability,
} from './driveView'

// The capability-eligible Drive picker (docs/storage.md): binds a storage role
// to a Drive whose provider advertises the required capability. Ineligible
// Drives stay visible but disabled with the missing capability named, so the
// capability rule reads as a fact of the system rather than a mystery.
type Props = {
  drives: DriveView[]
  requiredCapability: StorageCapability
  selectedDriveId: string | null
  onSelect: (driveId: string) => void
  label: string
  isDisabled?: boolean
}

const iconByKind = {
  [DriveKind.UNSPECIFIED]: LuFolder,
  [DriveKind.FS]: LuHardDrive,
  [DriveKind.S3]: LuCloud,
  [DriveKind.SFTP]: LuServer,
} as const satisfies Record<DriveKind, typeof LuFolder>

export function DrivePicker(props: Props) {
  const { t } = useTranslation()

  const { eligible, ineligible } = splitDrivesByCapability(props.drives, props.requiredCapability)
  const requiredLabel = t(capabilityLabelKeyByCapability[props.requiredCapability])

  return <Select
    label={props.label}
    isDisabled={props.isDisabled}
    selectedKeys={props.selectedDriveId ? [ props.selectedDriveId ] : []}
    disabledKeys={ineligible.map((drive) => drive.id)}
    onChange={(event) => {
      if (event.target.value) {
        props.onSelect(event.target.value)
      }
    }}
  >
    { [ ...eligible, ...ineligible ].map((drive) => {
      const KindIcon = iconByKind[drive.kind]
      const isEligible = eligible.includes(drive)
      return <SelectItem
        key={drive.id}
        textValue={drive.name}
        startContent={<KindIcon className='h-4 w-4 opacity-60' />}
        description={isEligible
          ? t(driveKindMeta[drive.kind].labelKey)
          : t('drive.picker.missingCapability', { capability: requiredLabel })}
      >
        { drive.isDefault
          ? t('drive.picker.defaultName', { name: drive.name })
          : drive.name }
      </SelectItem>
    }) }
  </Select>
}
