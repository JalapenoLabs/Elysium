// Copyright © 2026 Jalapeno Labs

import type { Key, UseOverlayStateReturn } from '@heroui/react'
import type { KeyboardEvent } from 'react'
import type { IssueReportSubject } from '../api/routes/issueReportRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import {
  Button,
  Description,
  Form,
  Label,
  Link,
  ListBox,
  Modal,
  Popover,
  Select,
  Spinner,
  TextArea,
  TextField,
  Tooltip,
  toast,
} from '@heroui/react'
import { LuCircleHelp, LuExternalLink } from 'react-icons/lu'

// Utility
import { HTTPError } from 'ky'

// Misc
import { getApiErrorCode, getApiErrorMessage, getUpstreamErrorMessage } from '../api/errors'
import { fileIssueReport, previewIssueReport } from '../api/routes/issueReportRoutes'
import { ISSUE_REPORT_NOTE_MAX_CHARACTERS, ISSUE_REPORT_PREVIEW_DELAY_MS } from '../constants'
import { useDebouncedValue } from '../hooks/useDebouncedValue'
import { getNewConnectorUrl } from '../urls'
import {
  blockerLabels,
  canTryToken,
  filedToastLabels,
  getReportBlocker,
  readinessLabels,
  resolveReportCredential,
} from './issueReportPresentation'

// The Markdown as it is posted, wrapped so a long line reads without scrolling sideways.
const PREVIEW_CLASS_NAME = 'max-h-72 overflow-auto rounded-md bg-default p-3 text-xs whitespace-pre-wrap break-words'

type Props = {
  state: UseOverlayStateReturn
  subject: IssueReportSubject
}

// Reports a problem to Elysium's GitHub repository as the person, with one of their own tokens.
// The preview is rendered by the API from the same inputs it posts, so what is shown is exactly
// what is posted; Post waits until the preview matches the note as typed.
export function ReportIssueModal(props: Props) {
  const { t } = useTranslation([ 'issueReports', 'common' ])
  const [ note, setNote ] = useState('')
  const [ pickedId, setPickedId ] = useState<string | null>(null)
  const [ isPosting, setIsPosting ] = useState(false)
  const typedNote = note.trim()
  const settledNote = useDebouncedValue(typedNote, ISSUE_REPORT_PREVIEW_DELAY_MS)

  const preview = useSWR(
    [ 'issue-report-preview', props.subject, settledNote ],
    ([ , subject, settled ]) => previewIssueReport({ subject, note: settled || undefined }),
    { revalidateOnFocus: false, shouldRetryOnError: false, keepPreviousData: true },
  )
  const credential = resolveReportCredential(preview.data, pickedId)
  const isCurrent = settledNote === typedNote && !preview.isValidating
  const blocker = getReportBlocker(preview.data, credential, isCurrent)

  async function post() {
    if (blocker || !credential) {
      console.debug('ReportIssueModal ignored a post it cannot make yet', { blocker })
      return
    }

    setIsPosting(true)
    try {
      const response = await fileIssueReport({
        subject: props.subject,
        note: typedNote || undefined,
        credentialId: credential.id,
      })
      const filed = response.report
      toast.success(t(filedToastLabels[filed.filedAs], { number: filed.issueNumber }), {
        description: <Link href={filed.url} target='_blank' rel='noreferrer' className='text-link'>
          <span>{t('toasts.view')}</span>
          <LuExternalLink className='size-3' aria-hidden />
        </Link>,
      })
      props.state.close()
    }
    catch (error) {
      console.debug('ReportIssueModal failed to file the report', { error })
      announceFailure(error)
    }
    finally {
      setIsPosting(false)
    }
  }

  function announceFailure(error: unknown) {
    const code = getApiErrorCode(error)
    if (code === 'already_reported') {
      toast.danger(t('errors.alreadyReported'))
      return
    }
    if (code === 'report_limit') {
      toast.danger(t('errors.limit'), { description: getApiErrorMessage(error) })
      return
    }
    if (code === 'github_token_cannot_report') {
      toast.danger(t('errors.tokenCannotReport'), { description: getApiErrorMessage(error) })
      return
    }
    const upstream = getUpstreamErrorMessage(error)
    toast.danger(t('common:errors.unexpected'), { description: upstream })
  }

  // What a preview that could not be rendered says. A run that expired or a connector that was
  // removed is gone; anything else, such as a check that passed after all, the API words.
  function describeLoadFailure(error: unknown) {
    if (error instanceof HTTPError && error.response.status === 404) {
      return t('errors.gone')
    }
    return getApiErrorMessage(error) ?? t('errors.loadFailed')
  }

  function onNoteKeyDown(event: KeyboardEvent) {
    // Enter is a new line in the note; Ctrl or Cmd with Enter posts, as comments do elsewhere.
    if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
      event.preventDefault()
      void post()
    }
  }

  const data = preview.data
  const previous = data?.previousReport

  return <Modal.Backdrop isOpen={props.state.isOpen} onOpenChange={props.state.setOpen}>
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-2xl'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{t('dialog.title')}</Modal.Heading>
        </Modal.Header>
        <Form
          onSubmit={(event) => {
            event.preventDefault()
            void post()
          }}
          validationBehavior='aria'
        >
          <Modal.Body className='mt-2 flex flex-col gap-4'>
            {preview.error && !data && <p className='text-sm text-danger'>{
              describeLoadFailure(preview.error)
            }</p>}

            {previous && <p className='text-sm'>
              <span>{t('dialog.previous', { when: new Date(previous.reportedAt).toLocaleString() })}</span>
              {' '}
              <Link href={previous.url} target='_blank' rel='noreferrer' className='text-link'>
                <span>{t('dialog.viewPrevious')}</span>
                <LuExternalLink className='size-3' aria-hidden />
              </Link>
            </p>}

            {data && !data.credentials.length && <div>
              <p className='compact text-sm'>{t('dialog.noToken')}</p>
              <Link href={getNewConnectorUrl('github')} className='text-link'>{t('dialog.addToken')}</Link>
            </div>}

            {data && Boolean(data.credentials.length) && <Select
              value={credential?.id ?? null}
              disabledKeys={data.credentials
                .filter((choice) => !canTryToken(choice.readiness))
                .map((choice) => choice.id)}
              onChange={(key: Key | null) => {
                if (key === null) {
                  console.debug('ReportIssueModal ignored an empty token choice')
                  return
                }
                setPickedId(String(key))
              }}
            >
              <Label>{t('dialog.token')}</Label>
              <Select.Trigger>
                <Select.Value />
                <Select.Indicator />
              </Select.Trigger>
              <Select.Popover>
                <ListBox>{
                  data.credentials.map((choice) => <ListBox.Item key={choice.id} id={choice.id} textValue={choice.name}>
                    <span className='min-w-0 truncate'>
                      {choice.name}
                      <span className='ml-2 text-xs opacity-60'>{choice.login}</span>
                      {choice.readiness !== 'ready' && choice.readiness !== 'fine_grained' && <span
                        className='ml-2 text-xs text-warning'
                      >{t(readinessLabels[choice.readiness])}</span>}
                    </span>
                    <ListBox.ItemIndicator />
                  </ListBox.Item>)
                }</ListBox>
              </Select.Popover>
              {credential && <Description>{
                credential.readiness === 'fine_grained'
                  ? t(readinessLabels.fine_grained)
                  : t('dialog.filedAs', { repository: data.repository, login: credential.login })
              }</Description>}
            </Select>}

            <TextField
              autoFocus
              maxLength={ISSUE_REPORT_NOTE_MAX_CHARACTERS}
              value={note}
              onChange={setNote}
            >
              <Label>{t('dialog.note')}</Label>
              <TextArea rows={3} onKeyDown={onNoteKeyDown} />
            </TextField>

            <div>
              <div className='compact flex items-center gap-1'>
                <p className='text-sm font-medium'>{t('dialog.preview')}</p>
                <Popover>
                  <Button isIconOnly size='sm' variant='ghost' aria-label={t('dialog.preview')}>
                    <LuCircleHelp className='size-4' aria-hidden />
                  </Button>
                  <Popover.Content placement='top' className='max-w-xs'>
                    <Popover.Dialog>
                      <p className='text-sm'>{t('dialog.previewHint')}</p>
                    </Popover.Dialog>
                  </Popover.Content>
                </Popover>
                {!isCurrent && <Spinner size='sm' className='ml-1' />}
              </div>
              {data
                ? <pre className={PREVIEW_CLASS_NAME}>
                  <strong>{data.draft.title}</strong>
                  {'\n\n'}
                  {data.draft.body}
                </pre>
                : !preview.error && <p className='text-sm opacity-70'>{t('dialog.loading')}</p>}
              {data && Boolean(data.credentials.length) && <p className='mt-1 text-xs opacity-60'>{
                t('dialog.remaining', { count: data.remainingToday })
              }</p>}
            </div>
          </Modal.Body>
          <Modal.Footer>
            <Button slot='close' variant='tertiary'>
              <span>{t('common:actions.cancel')}</span>
            </Button>
            <Tooltip delay={200} isDisabled={!blocker}>
              <Tooltip.Trigger>
                <div>
                  <Button type='submit' isDisabled={Boolean(blocker)} isPending={isPosting}>
                    <span>{t('dialog.post')}</span>
                  </Button>
                </div>
              </Tooltip.Trigger>
              <Tooltip.Content>{blocker && t(blockerLabels[blocker])}</Tooltip.Content>
            </Tooltip>
          </Modal.Footer>
        </Form>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}
