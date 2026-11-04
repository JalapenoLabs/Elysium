// Copyright © 2026 Jalapeno Labs

import type { CompanyDocProposal } from '../../../api/routes/companyDocRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { companyDocLoaded } from '../../../store/companyDocSlice'
import { useAppDispatch } from '../../../store/hooks'

// User interface
import { Button, Card, Link, Tooltip } from '@heroui/react'
import { LuCheck, LuX } from 'react-icons/lu'
import { TextDiff } from '../../../components/TextDiff'
import { DateText } from '../../../format/DateText'

// Misc
import { getApiErrorCode, getApiErrorMessage } from '../../../api/errors'
import { approveCompanyDocProposal, rejectCompanyDocProposal } from '../../../api/routes/companyDocRoutes'
import { useUserNames } from '../../../hooks/useUserNames'
import { getRunViewUrl } from '../../../urls'
import { describeAgent, sectionTitleKeys } from './companyDocPresentation'
import { toast } from '../../../toast/toast'

type Props = {
  proposal: CompanyDocProposal
  // The section as it reads now, so a proposal made against an earlier version says so.
  currentContent: string
  // Whether the person has unsaved edits to this section, which approving would collide with.
  hasUnsavedEdits: boolean
}

// One proposed change to a section: who proposed it and why, what it changes as a diff, and the
// words it was read in; Approve and Reject act at once.
export function ProposalCard(props: Props) {
  const { t } = useTranslation([ 'companyDoc', 'common' ])
  const dispatch = useAppDispatch()
  const userNames = useUserNames()
  const [ deciding, setDeciding ] = useState<'approve' | 'reject' | null>(null)
  const proposal = props.proposal

  async function decide(decision: 'approve' | 'reject') {
    setDeciding(decision)
    try {
      const response = decision === 'approve'
        ? await approveCompanyDocProposal(proposal.id)
        : await rejectCompanyDocProposal(proposal.id)
      dispatch(companyDocLoaded(response.doc))
      toast.success(decision === 'approve'
        ? t('toasts.approved')
        : t('toasts.rejected'))
    }
    catch (error) {
      console.debug('ProposalCard could not decide a proposal', { error, decision, proposalId: proposal.id })
      // The section changed in the same lines since the proposal was made; it stays pending.
      if (getApiErrorCode(error) === 'proposal_overlaps') {
        toast.warning(t('toasts.overlaps'))
        return
      }
      toast.danger(t('common:errors.unexpected'), { description: getApiErrorMessage(error) ?? undefined })
    }
    finally {
      setDeciding(null)
    }
  }

  const who = userNames[proposal.createdBy] ?? t('common:table.someone')
  const agent = proposal.agent && describeAgent(proposal.agent)
  const agentName = agent?.kind === 'session'
    ? t('proposal.sessionAgent', { number: agent.number })
    : agent?.name ?? t('proposal.unnamedAgent')
  const byline = agent
    ? t('proposal.proposedByAgent', { who, agent: agentName })
    : t('proposal.proposedBy', { who })
  const approveButton = <Button
    size='sm'
    variant='primary'
    isDisabled={deciding !== null || props.hasUnsavedEdits}
    isPending={deciding === 'approve'}
    onPress={() => decide('approve')}
  >
    <LuCheck className='size-4' aria-hidden />
    <span>{t('proposal.approve')}</span>
  </Button>

  return <Card className='compact'>
    <Card.Content className='flex flex-col gap-3'>
      <div className='flex flex-wrap items-center gap-x-2 text-xs opacity-70'>
        <span>{byline}</span>
        <span aria-hidden>·</span>
        <DateText value={proposal.createdAt} />
        {proposal.workflowRunId && <>
          <span aria-hidden>·</span>
          <Link href={getRunViewUrl(proposal.workflowRunId)} className='text-link text-xs'>{
            t('proposal.fromReview')
          }</Link>
        </>}
      </div>

      <p className='text-sm whitespace-pre-line'>{proposal.reason}</p>

      <TextDiff
        before={proposal.baseContent}
        after={proposal.content}
        label={t('proposal.diffLabel', { section: t(sectionTitleKeys[proposal.section]) })}
        foldedLabel={(count) => t('proposal.foldedLines', { count })}
      />
      {proposal.baseContent !== props.currentContent && <p className='text-xs opacity-70'>{
        t('proposal.earlierVersion')
      }</p>}

      {(proposal.quote || proposal.source) && <div className='text-sm'>
        {proposal.quote && <blockquote className='border-l-2 border-separator pl-3 italic opacity-80'>{
          proposal.quote
        }</blockquote>}
        {proposal.source && <p className='mt-1 text-xs opacity-60'>{proposal.source}</p>}
      </div>}

      <div className='flex flex-wrap items-center gap-2'>
        {props.hasUnsavedEdits
          // A disabled button fires no hover, so its tooltip hangs on a wrapper.
          ? <Tooltip delay={300}>
            <Tooltip.Trigger>
              <div>{approveButton}</div>
            </Tooltip.Trigger>
            <Tooltip.Content>
              <span>{t('proposal.saveFirst')}</span>
            </Tooltip.Content>
          </Tooltip>
          : approveButton}
        <Button
          size='sm'
          variant='outline'
          isDisabled={deciding !== null}
          isPending={deciding === 'reject'}
          onPress={() => decide('reject')}
        >
          <LuX className='size-4' aria-hidden />
          <span>{t('proposal.reject')}</span>
        </Button>
      </div>
    </Card.Content>
  </Card>
}
