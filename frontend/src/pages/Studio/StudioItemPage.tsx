// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams, useSearchParams } from 'react-router'

// Redux
import { useAppDispatch, useAppSelector } from '../../store/hooks'
import { turnFocusRequested } from '../../store/sessionEventsSlice'
import { selectStudioItemById } from '../../store/studioItemsSlice'

// User interface
import { Link, Spinner } from '@heroui/react'
import { StudioItemWorkspace } from './StudioItemWorkspace'

// Misc
import { useStudioItemLoader } from '../../hooks/useServerData'
import { FOCUS_TURN_PARAM, UrlTree } from '../../urls'

// `/studio/:itemId`: one item, live or deleted. Loads it, then hands it to the workspace,
// which can rely on it being there. `?turn=<id>` opens its conversation at that turn, as a turn
// summary on a project's timeline links to it.
export function StudioItemPage() {
  const { t } = useTranslation('studio')
  const dispatch = useAppDispatch()
  const itemId = useParams().itemId ?? ''
  const [ searchParams, setSearchParams ] = useSearchParams()
  const requestedTurnId = searchParams.get(FOCUS_TURN_PARAM)
  const status = useStudioItemLoader(itemId)
  const item = useAppSelector((state) => selectStudioItemById(state, itemId))

  // Asked for once; the address drops it, so a reload lands on the latest message as usual.
  useEffect(() => {
    if (!requestedTurnId) {
      return
    }
    dispatch(turnFocusRequested(requestedTurnId))
    setSearchParams((params) => {
      params.delete(FOCUS_TURN_PARAM)
      return params
    }, { replace: true })
  }, [ requestedTurnId, dispatch, setSearchParams ])

  if (item) {
    return <StudioItemWorkspace item={item} />
  }

  if (status === 'loading') {
    return <div className='grid h-full place-items-center'>
      <Spinner />
    </div>
  }

  // Failing to load is most often an item deleted for good, which answers 404; one deleted
  // for good while shown leaves Redux with the load still standing.
  return <div className='grid h-full place-items-center p-6'>
    <div className='text-center'>
      <p className='compact text-sm opacity-70'>{t('item.missing')}</p>
      <Link href={UrlTree.studio} className='text-link'>{t('item.back')}</Link>
    </div>
  </div>
}
