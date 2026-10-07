// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectPeopleByStanding } from '../../store/usersSlice'

// User interface
import { Chip } from '@heroui/react'
import { LuUsers } from 'react-icons/lu'
import { SettingsDirectoryItem } from './SettingsDirectoryItem'

// Misc
import { useUsersLoader } from '../../hooks/useServerData'
import { UrlTree } from '../../urls'

// The Users entry, for admins, counting the sign-ups waiting for them.
export function UsersDirectoryItem() {
  const { t } = useTranslation('settings')
  useUsersLoader()
  const pendingCount = useAppSelector(selectPeopleByStanding).pending.length

  return <SettingsDirectoryItem
    icon={LuUsers}
    title={t('items.users.title')}
    description={t('items.users.description')}
    href={UrlTree.settingsUsers}
    badge={pendingCount > 0 && <Chip size='sm' variant='soft' color='warning'>{
      t('items.users.pending', { count: pendingCount })
    }</Chip>}
  />
}
