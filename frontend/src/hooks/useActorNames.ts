// Copyright © 2026 Jalapeno Labs

import type { ActorNames } from '../pages/ActionItems/actionItemPresentation'

// Core
import { shallowEqual } from 'react-redux'

// Redux
import { selectMe } from '../store/authSlice'
import { useAppSelector } from '../store/hooks'
import { selectUserNamesById } from '../store/usersSlice'

// Misc
import { useUsersLoader } from './useServerData'

// Everyone's names and who is reading, for views that say who did or wrote something.
export function useActorNames(): ActorNames {
  useUsersLoader()
  const namesById = useAppSelector(selectUserNamesById, shallowEqual)
  const meId = useAppSelector(selectMe)?.user.id ?? null
  return { namesById, meId }
}
