// Copyright © 2026 Jalapeno Labs

import type { AppDispatch, RootState } from './index'

// Core
import { useDispatch, useSelector } from 'react-redux'

// Typed once here so components never import RootState. Selectors must return
// existing references, never new objects, or every dispatch re-renders the caller.
export const useAppDispatch = useDispatch.withTypes<AppDispatch>()
export const useAppSelector = useSelector.withTypes<RootState>()
