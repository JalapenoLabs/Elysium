// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import {
  selectPasswordVault,
  selectPasswordVaultLoaded,
  selectPasswordVaultStatus,
} from '../../../store/passwordVaultSlice'

// User interface
import { Spinner } from '@heroui/react'
import { CollectionPicker } from './CollectionPicker'
import { PasswordVaultForm } from './PasswordVaultForm'
import { PasswordVaultOverview } from './PasswordVaultOverview'

// Misc
import { usePasswordVaultLoader } from '../../../hooks/useServerData'

// Loads the vault once; the event stream keeps it and its status current from then on. With
// no vault, or while an admin changes its credentials, the form shows instead of the overview.
export function PasswordVaultAdministration() {
  const { t } = useTranslation('passwords')
  const loadStatus = usePasswordVaultLoader()
  const isLoaded = useAppSelector(selectPasswordVaultLoaded)
  const vault = useAppSelector(selectPasswordVault)
  const status = useAppSelector(selectPasswordVaultStatus)
  const [ isEditing, setIsEditing ] = useState(false)

  if (!isLoaded || !status) {
    return loadStatus === 'failed'
      ? <p className='py-10 text-center text-sm text-danger'>{t('loadError')}</p>
      : <div className='grid place-items-center py-16'>
        <Spinner />
      </div>
  }

  if (!vault || isEditing) {
    return <>
      <p className='relaxed max-w-2xl text-sm opacity-70'>{t('description')}</p>
      <PasswordVaultForm
        vault={vault}
        onSaved={() => setIsEditing(false)}
        onCancel={vault ? () => setIsEditing(false) : null}
      />
    </>
  }

  return <>
    <p className='relaxed max-w-2xl text-sm opacity-70'>{t('description')}</p>
    <PasswordVaultOverview
      vault={vault}
      status={status}
      onEdit={() => setIsEditing(true)}
    />
    <CollectionPicker
      vault={vault}
      status={status}
    />
  </>
}
