// Copyright © 2026 Jalapeno Labs

import type { RootState } from './index'

// Core
import { useSelector } from 'react-redux'

// Pre-typed selector hook so components never import RootState directly. To
// dispatch, import the store's `dispatch` directly (`import { dispatch } from
// '../store'`): it is stable and needs no hook.
export const useAppSelector = useSelector.withTypes<RootState>()
