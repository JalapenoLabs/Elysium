// Copyright © 2026 Jalapeno Labs

import type { Finding } from '../../api/routes/findingRoutes'

// Core
import { useMemo, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import { mutate } from 'swr'

// Misc
import { getApiErrorCode } from '../../api/errors'
import {
  acceptFinding,
  dismissFinding,
  DISMISSAL_REASON_MAX_CHARACTERS,
  projectFindingsKey,
} from '../../api/routes/findingRoutes'
import { getProjectManagerKey } from '../../api/routes/projectManagerRoutes'
import { usePrompt } from '../../hooks/usePrompt'
import { toast } from '../../toast/toast'
import { getWorkBatchViewUrl } from '../../urls'
import { providerLabelKeys } from '../ActionItems/linkPresentation'

// The finding's list and its project's Project Manager, whose Proposed tab gains the batch.
function reload(finding: Finding) {
  void mutate(projectFindingsKey(finding.projectId))
  void mutate(getProjectManagerKey(finding.projectId))
}

// What a person does with a finding from its project's Findings tab. Accepting makes its item,
// owes its issue, and proposes its batch, which still waits for their approval, so the toast
// offers the batch. Dismissing asks first, for an optional reason the agents that see it again are
// told, since it is final. Each reloads the list and the Project Manager's tabs, so this tab
// moves before the event arrives, and reports its own failure. The actions keep their identity
// across renders, so a table memoizing its row actions on them holds.
export function useFindingActions() {
  const { t } = useTranslation([ 'projects', 'actionItems', 'common' ])
  const navigate = useNavigate()
  const prompt = usePrompt()
  // The findings being accepted now: a second press while the first is in flight sends nothing,
  // rather than being refused as someone else's decision.
  const accepting = useRef(new Set<string>())

  return useMemo(() => {
    function report(error: unknown, finding: Finding, action: 'accept' | 'dismiss') {
      // Someone decided it in another tab, or another person did, since this list was drawn.
      if (getApiErrorCode(error) === 'finding_decided') {
        toast.danger(t('findings.decidedElsewhere'))
        reload(finding)
        return
      }
      console.debug(`useFindingActions could not ${action} a finding`, { error, findingId: finding.id })
      toast.danger(t('common:errors.unexpected'))
    }

    async function accept(finding: Finding) {
      if (accepting.current.has(finding.id)) {
        console.debug('useFindingActions ignored a second accept while the first is in flight', {
          findingId: finding.id,
        })
        return
      }
      accepting.current.add(finding.id)
      try {
        const accepted = await acceptFinding(finding.id)
        reload(finding)
        const reviewBatch = {
          label: t('findings.reviewBatch'),
          onPress: () => navigate(getWorkBatchViewUrl(accepted.batchId)),
        }
        // A project with nowhere to file still gets its item and batch. The API says why no
        // issue was owed (no tracker, or a GitHub tracker with no token) and how to fix it, and
        // the warning stays until the person reads it.
        if (!accepted.issue) {
          toast.warning(t('findings.acceptedNowhereToFile', { title: finding.title }), {
            description: t('findings.nowhereToFile', { reason: accepted.issueRefusal ?? '' }),
            action: reviewBatch,
          })
          return
        }
        toast.success(t('findings.accepted', { title: finding.title }), {
          description: t('findings.acceptedFiling', {
            provider: t(`actionItems:${providerLabelKeys[accepted.issue.provider]}`),
            destination: accepted.issue.destination,
          }),
          action: reviewBatch,
        })
      }
      catch (error) {
        report(error, finding, 'accept')
      }
      finally {
        accepting.current.delete(finding.id)
      }
    }

    function dismiss(finding: Finding) {
      prompt({
        title: t('findings.dismissPrompt.title', { title: finding.title }),
        message: <p>
          <strong>{t('findings.dismissPrompt.final')}</strong>
        </p>,
        label: t('findings.dismissPrompt.reason'),
        isOptional: true,
        isMultiline: true,
        maxLength: DISMISSAL_REASON_MAX_CHARACTERS,
        submitText: t('findings.actions.dismiss'),
        cancelText: t('findings.dismissPrompt.keep'),
        onSubmit: async (reason) => {
          try {
            await dismissFinding(finding.id, reason || null)
            reload(finding)
            toast.success(t('findings.dismissed', { title: finding.title }))
          }
          catch (error) {
            report(error, finding, 'dismiss')
            // A finding someone else decided cannot be dismissed by trying again, so the dialog
            // closes; anything else keeps it open with the reason as typed.
            if (getApiErrorCode(error) !== 'finding_decided') {
              throw error
            }
          }
        },
      })
    }

    return { accept, dismiss } as const
  }, [ t, navigate, prompt ])
}
