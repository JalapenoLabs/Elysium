// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectActionItemById } from '../../store/actionItemsSlice'

// Misc
import { providerLabelKeys } from '../ActionItems/linkPresentation'

type Props = {
  itemId: string
  // The filing the operation owed, as its result names it.
  filingId: string
}

// The issue an applied file-issue still owes, while the watcher has not filed it: waiting, or
// with its tracker's last answer. Read from the item, which the event stream keeps current;
// once filed, the issue is one of the item's links and nothing shows here.
export function OperationIssueFiling(props: Props) {
  const { t } = useTranslation([ 'changesets', 'actionItems' ])
  const filing = useAppSelector((state) => selectActionItemById(state, props.itemId)?.issueFiling)

  if (!filing || filing.id !== props.filingId) {
    return null
  }
  const provider = t(providerLabelKeys[filing.provider], { ns: 'actionItems' })
  return <p className='mt-2 text-sm text-warning'>{
    filing.lastError
      ? t('review.providerFailed', { provider, error: filing.lastError })
      : t('review.providerWaiting', { provider })
  }</p>
}
