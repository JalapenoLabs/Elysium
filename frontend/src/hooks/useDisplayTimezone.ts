// Copyright © 2026 Jalapeno Labs

// Redux
import { useAppSelector } from '../store/hooks'

// Misc
import { resolveDisplayTimezone } from '../proto/helpers/timezone'

// The zone the signed-in viewer's timestamps should render in: their saved
// preference, or the browser's zone when that preference is empty (auto-detect,
// the default). Read this once per view and pass it down to the timestamps,
// rather than letting each timestamp reach for global state itself.
export function useDisplayTimezone(): string {
  const userTimezone = useAppSelector((state) => state.currentUser.user?.timezone)
  return resolveDisplayTimezone(userTimezone)
}
