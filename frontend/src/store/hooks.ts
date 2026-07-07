// Copyright © 2026 Jalapeno Labs

import type { RootState, AppDispatch } from './index'

// Core
import { useDispatch, useSelector } from 'react-redux'

// Pre-typed hooks so components never import RootState/AppDispatch directly.
export const useAppDispatch = useDispatch.withTypes<AppDispatch>()
export const useAppSelector = useSelector.withTypes<RootState>()
