// Copyright © 2026 Jalapeno Labs

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectActionItemHistory } from '../../store/actionItemHistorySlice'

// User interface
import { HistoryTimeline } from './HistoryTimeline'

// Misc
import { useActionItemHistoryLoader } from '../../hooks/useServerData'
import { useUndoActionItemChange } from './useUndoActionItemChange'

type Props = {
  itemId: string
}

// An item's history, loaded once and kept current by `history.appended`. A change to its projects
// or its lead is undone from here.
export function ActionItemHistory(props: Props) {
  const status = useActionItemHistoryLoader(props.itemId)
  const entries = useAppSelector((state) => selectActionItemHistory(state, props.itemId))
  const undo = useUndoActionItemChange()

  return <HistoryTimeline
    entries={entries}
    subject='item'
    status={status}
    onUndo={(entry) => undo(props.itemId, entry.id)}
  />
}
