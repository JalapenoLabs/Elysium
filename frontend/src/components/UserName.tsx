// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../store/hooks'
import { selectUserById } from '../store/usersSlice'

// Misc
import { useUsersLoader } from '../hooks/useServerData'

type Props = {
  userId: string
}

// The name of whoever `userId` is: a person, or a machine such as the coding agent. Used
// for "created by" and "decided by".
export function UserName(props: Props) {
  const { t } = useTranslation('common')
  useUsersLoader()
  const user = useAppSelector((state) => selectUserById(state, props.userId))

  return <span>{user?.name ?? t('table.someone')}</span>
}
