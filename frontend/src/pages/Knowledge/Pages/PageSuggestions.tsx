// Copyright © 2026 Jalapeno Labs

import type { KnowledgePage, PageSuggestion } from '../../../api/routes/pageRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { pageLoaded } from '../../../store/pagesSlice'

// User interface
import { Spinner } from '@heroui/react'
import { PageSuggestionCard } from './PageSuggestionCard'

// Misc
import { getApiErrorCode, getApiErrorMessage } from '../../../api/errors'
import {
  acceptPageSuggestion,
  dismissPageSuggestion,
  listPageSuggestions,
  pageSuggestionsKey,
} from '../../../api/routes/pageRoutes'
import { PAGE_SUGGESTION_REASON_MAX_CHARACTERS } from '../../../constants'
import { usePrompt } from '../../../hooks/usePrompt'
import { toast } from '../../../toast/toast'

type Props = {
  page: KnowledgePage
}

// The refusals a decision can meet because someone else got there first, each told in its own words
// and answered by loading the suggestions again, since what they show is behind.
const refusalKeys = {
  suggestion_overlaps: 'pages.suggestions.toasts.overlaps',
  suggestion_decided: 'pages.suggestions.toasts.decided',
} as const

// A page's Suggestions tab: the edits agents suggested to it, oldest first, each accepted wholly or
// in part or dismissed with a reason. Read from SWR alone, since no other view shows them, and
// loaded again on `page.changed`, which every suggestion made or decided announces.
export function PageSuggestions(props: Props) {
  const { t } = useTranslation([ 'knowledge', 'common' ])
  const dispatch = useAppDispatch()
  const prompt = usePrompt()
  const pageId = props.page.id
  const suggestions = useSWR(pageSuggestionsKey(pageId), () => listPageSuggestions(pageId))
  const [ isDeciding, setIsDeciding ] = useState(false)

  function refused(error: unknown) {
    const code = getApiErrorCode(error)
    if (code === 'suggestion_overlaps' || code === 'suggestion_decided') {
      toast.warning(t(refusalKeys[code]))
      void suggestions.mutate()
      return
    }
    toast.danger(t('common:errors.unexpected'), { description: getApiErrorMessage(error) ?? undefined })
  }

  async function accept(suggestion: PageSuggestion, changes: number[] | undefined) {
    setIsDeciding(true)
    try {
      const response = await acceptPageSuggestion(pageId, suggestion.id, changes)
      dispatch(pageLoaded(response.page))
      toast.success(t('pages.suggestions.toasts.accepted'))
      await suggestions.mutate()
    }
    catch (error) {
      console.debug('PageSuggestions could not accept a suggestion', { error, suggestionId: suggestion.id })
      refused(error)
    }
    finally {
      setIsDeciding(false)
    }
  }

  function dismiss(suggestion: PageSuggestion) {
    prompt({
      title: t('pages.suggestions.dismissPrompt.title'),
      label: t('pages.suggestions.dismissPrompt.reason'),
      isMultiline: true,
      maxLength: PAGE_SUGGESTION_REASON_MAX_CHARACTERS,
      submitText: t('pages.suggestions.dismiss'),
      cancelText: t('pages.suggestions.dismissPrompt.keep'),
      onSubmit: async (reason) => {
        setIsDeciding(true)
        try {
          const response = await dismissPageSuggestion(pageId, suggestion.id, reason)
          dispatch(pageLoaded(response.page))
          toast.success(t('pages.suggestions.toasts.dismissed'))
          await suggestions.mutate()
        }
        catch (error) {
          console.debug('PageSuggestions could not dismiss a suggestion', { error, suggestionId: suggestion.id })
          refused(error)
          // A refusal that is someone else's decision closes the dialog; anything else keeps it
          // open with the reason typed, to try again.
          if (getApiErrorCode(error) !== 'suggestion_decided') {
            throw error
          }
        }
        finally {
          setIsDeciding(false)
        }
      },
    })
  }

  if (suggestions.error && !suggestions.data) {
    return <p className='py-10 text-center text-sm text-danger'>{t('pages.suggestions.loadFailed')}</p>
  }
  if (!suggestions.data) {
    return <div className='grid place-items-center py-16'>
      <Spinner />
    </div>
  }
  if (!suggestions.data.suggestions.length) {
    return <p className='py-10 text-center text-sm opacity-70'>{t('pages.suggestions.none')}</p>
  }

  return <div className='max-w-3xl'>{
    suggestions.data.suggestions.map((suggestion) => <PageSuggestionCard
      // A new version can change which of its changes still merge, so its choice starts over.
      key={`${suggestion.id}:${props.page.currentVersion}`}
      suggestion={suggestion}
      page={props.page}
      isDeciding={isDeciding}
      onAccept={(chosen, changes) => void accept(chosen, changes)}
      onDismiss={dismiss}
    />)
  }</div>
}
