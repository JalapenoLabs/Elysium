// Copyright © 2026 Jalapeno Labs

// Core
import { shallowEqual } from 'react-redux'

// Redux
import { useAppSelector } from '../store/hooks'
import { selectUserNamesById } from '../store/usersSlice'

// Misc
import { useUsersLoader } from './useServerData'

// Every user's name by id, loaded as needed, for tables with a "Created by" column.
export function useUserNames() {
  useUsersLoader()
  return useAppSelector(selectUserNamesById, shallowEqual)
}
