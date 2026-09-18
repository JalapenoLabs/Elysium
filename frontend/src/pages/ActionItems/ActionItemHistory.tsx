// Copyright © 2026 Jalapeno Labs

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectActionItemHistory } from '../../store/actionItemHistorySlice'

// User interface
import { HistoryTimeline } from './HistoryTimeline'

// Misc
import { useActionItemHistoryLoader } from '../../hooks/useServerData'

type Props = {
  itemId: string
}

// An item's history, loaded once and kept current by `history.appended`.
export function ActionItemHistory(props: Props) {
  const status = useActionItemHistoryLoader(props.itemId)
  const entries = useAppSelector((state) => selectActionItemHistory(state, props.itemId))

  return <HistoryTimeline
    entries={entries}
    subject='item'
    status={status}
  />
}
