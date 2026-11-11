// Copyright © 2026 Jalapeno Labs

import type { KnowledgePage, PageSuggestion } from '../../../api/routes/pageRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Card, Checkbox, Link, Tooltip } from '@heroui/react'
import { LuCheck, LuTriangleAlert, LuX } from 'react-icons/lu'
import { TextDiff } from '../../../components/TextDiff'
import { DateText } from '../../../format/DateText'

// Misc
import { useUserNames } from '../../../hooks/useUserNames'
import { getRunViewUrl } from '../../../urls'
import { pageAgentName } from './pageHistoryPresentation'
import { acceptLabel, initialChoice, toggleChoice } from './pageSuggestionPresentation'

type Props = {
  suggestion: PageSuggestion
  page: KnowledgePage
  // Whether a decision on any suggestion of the page is under way, which holds every card's
  // buttons, since one decision changes what the others merge with.
  isDeciding: boolean
  onAccept: (suggestion: PageSuggestion, changes: number[] | undefined) => void
  onDismiss: (suggestion: PageSuggestion) => void
}

// One edit an agent suggested: who suggested it and why, each of its changes as a diff of the lines
// around it, a box to take each when there are several, and the words it was read in. Accept takes
// the changes ticked and Dismiss asks why; both act at once.
export function PageSuggestionCard(props: Props) {
  const { t } = useTranslation([ 'knowledge', 'common' ])
  const userNames = useUserNames()
  const { suggestion, page } = props
  const [ chosen, setChosen ] = useState(() => initialChoice(suggestion.changes))
  const hasChoice = suggestion.changes.length > 1

  const who = userNames[suggestion.createdBy] ?? t('common:table.someone')
  const byline = suggestion.agent
    ? t('pages.suggestions.suggestedByAgent', { who, agent: pageAgentName(suggestion.agent, t) })
    : t('pages.suggestions.suggestedBy', { who })
  const label = acceptLabel(chosen, suggestion.changes.length)
  // Why Accept waits, most fundamental first: nothing can be taken from a deleted page, nor a change
  // whose lines the page changed since, and nothing is taken while nothing is ticked.
  let blockedReason: string | null = null
  if (page.deletedAt) {
    blockedReason = t('pages.suggestions.deletedPage')
  }
  else if (!suggestion.changes.some((change) => change.applies)) {
    blockedReason = t('pages.suggestions.nothingApplies')
  }
  else if (!chosen.size) {
    blockedReason = t('pages.suggestions.chooseOne')
  }
  const acceptButton = <Button
    size='sm'
    variant='primary'
    isDisabled={props.isDeciding || blockedReason !== null}
    onPress={() => props.onAccept(suggestion, label.changes)}
  >
    <LuCheck className='size-4' aria-hidden />
    <span>{label.kind === 'all'
      ? t('pages.suggestions.accept')
      : t('pages.suggestions.acceptSome', { taken: label.taken, count: label.count })}</span>
  </Button>

  return <Card className='relaxed'>
    <Card.Content className='flex flex-col gap-3'>
      <div className='flex flex-wrap items-center gap-x-2 text-xs opacity-70'>
        <span>{byline}</span>
        <span aria-hidden>·</span>
        <DateText value={suggestion.createdAt} />
        {suggestion.workflowRunId && <>
          <span aria-hidden>·</span>
          <Link href={getRunViewUrl(suggestion.workflowRunId)} className='text-link text-xs'>{
            t('pages.suggestions.fromReview')
          }</Link>
        </>}
      </div>

      <p className='text-sm whitespace-pre-line'>{suggestion.reason}</p>

      {(suggestion.quote || suggestion.source) && <div className='text-sm'>
        {suggestion.quote && <blockquote className='border-l-2 border-separator pl-3 italic opacity-80'>{
          suggestion.quote
        }</blockquote>}
        {suggestion.source && <p className='mt-1 text-xs opacity-60'>{suggestion.source}</p>}
      </div>}

      {suggestion.baseVersion !== page.currentVersion && <p className='text-xs opacity-70'>{
        t('pages.suggestions.earlierVersion', { version: suggestion.baseVersion })
      }</p>}

      <ul className='flex flex-col gap-3'>{
        suggestion.changes.map((change) => <li key={change.index} className='flex flex-col gap-1'>
          <div className='flex flex-wrap items-center gap-2 text-xs'>
            {hasChoice
              ? <Checkbox
                id={`suggestion-${suggestion.id}-change-${change.index}`}
                aria-label={t('pages.suggestions.take', { line: change.line })}
                isSelected={chosen.has(change.index)}
                isDisabled={!change.applies || props.isDeciding}
                onChange={() => setChosen((current) => toggleChoice(current, change.index))}
              >
                <Checkbox.Content>
                  <Checkbox.Control>
                    <Checkbox.Indicator />
                  </Checkbox.Control>
                  <span className='opacity-70'>{t('pages.suggestions.line', { line: change.line })}</span>
                </Checkbox.Content>
              </Checkbox>
              : <span className='opacity-70'>{t('pages.suggestions.line', { line: change.line })}</span>}
            {!change.applies && <span className='flex items-center gap-1 text-warning'>
              <LuTriangleAlert className='size-3.5 shrink-0' aria-hidden />
              <span>{t('pages.suggestions.changedSince')}</span>
            </span>}
          </div>
          <TextDiff
            before={change.before}
            after={change.after}
            label={t('pages.suggestions.diffLabel', { line: change.line })}
            foldedLabel={(count) => t('pages.suggestions.foldedLines', { count })}
          />
        </li>)
      }</ul>

      <div className='flex flex-wrap items-center gap-2'>
        {/* A disabled button hears no hover, so its tooltip hangs on a wrapper that stays put. */}
        <Tooltip delay={300} isDisabled={blockedReason === null}>
          <Tooltip.Trigger>
            <div>{acceptButton}</div>
          </Tooltip.Trigger>
          <Tooltip.Content>
            <span>{blockedReason}</span>
          </Tooltip.Content>
        </Tooltip>
        <Button
          size='sm'
          variant='outline'
          isDisabled={props.isDeciding}
          onPress={() => props.onDismiss(suggestion)}
        >
          <LuX className='size-4' aria-hidden />
          <span>{t('pages.suggestions.dismiss')}</span>
        </Button>
      </div>
    </Card.Content>
  </Card>
}
