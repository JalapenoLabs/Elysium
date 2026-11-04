// Copyright © 2026 Jalapeno Labs

import type { IssueReportSubject } from '../api/routes/issueReportRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Tooltip } from '@heroui/react'
import { LuFlag } from 'react-icons/lu'

// Misc
import { useReportIssue } from '../hooks/useReportIssue'

type Props = {
  subject: IssueReportSubject
  // Just the flag, where a row has no room for the word; the tooltip still names it.
  isIconOnly?: boolean
}

// Report, beside an error the person believes is not theirs to fix. Any surface that shows an
// error can drop it in with what the error is about.
export function ReportIssueButton(props: Props) {
  const { t } = useTranslation('issueReports')
  const reportIssue = useReportIssue()

  return <Tooltip delay={400}>
    <Button
      size='sm'
      variant='ghost'
      isIconOnly={props.isIconOnly}
      aria-label={props.isIconOnly
        ? t('button.tooltip')
        : undefined}
      onPress={() => reportIssue(props.subject)}
    >
      <LuFlag className='size-4' aria-hidden />
      {!props.isIconOnly && <span>{t('button.label')}</span>}
    </Button>
    <Tooltip.Content>{t('button.tooltip')}</Tooltip.Content>
  </Tooltip>
}
