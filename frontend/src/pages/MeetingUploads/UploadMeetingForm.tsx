// Copyright © 2026 Jalapeno Labs

import type { Meeting } from '../../api/routes/meetingRoutes'
import type {
  CreateMeetingUploadRequest,
  MeetingUpload,
  MeetingUploadFileKind,
  MeetingUploadPolicy,
} from '../../api/routes/meetingUploadRoutes'
import type { ChosenFiles, UploadDetails } from './meetingUpload'
import type { MeetingUploadFormValues } from './meetingUploadFormSchema'
import type { ParseKeys } from 'i18next'

// Core
import { useEffect, useMemo, useRef, useState } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// Redux
import { useAppDispatch, useAppSelector } from '../../store/hooks'
import { meetingUpserted } from '../../store/meetingsSlice'
import { selectAllStorageLocations } from '../../store/storageLocationsSlice'

// User interface
import { FieldError, Form, Input, Label, Link, ProgressBar, TextField } from '@heroui/react'
import { OptionSelect } from '../../components/OptionSelect'
import { PageCreateFooter } from '../../layout/page/PageCreateFooter'
import { MeetingFileField } from './MeetingFileField'
import { MeetingStartField } from './MeetingStartField'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'

// Misc
import { getApiErrorCode, getApiErrorMessage } from '../../api/errors'
import {
  MEETING_UPLOAD_FILE_KINDS,
  cancelMeetingUpload,
  createMeetingUpload,
  finishMeetingUpload,
  getMeetingUpload,
} from '../../api/routes/meetingUploadRoutes'
import { useFormDraft } from '../../drafts/useFormDraft'
import { formatBytes } from '../../format/numbers'
import { useInitialFocus } from '../../hooks/useInitialFocus'
import { saveForm, usePageSave } from '../../layout/page/pageSave'
import { toast } from '../../toast/toast'
import { UrlTree, getMeetingUrl } from '../../urls'
import { findResumable, titleFromFileName, uploadProgress } from './meetingUpload'
import { createMeetingUploadFormSchema } from './meetingUploadFormSchema'
import { sendMeetingFiles } from './sendMeetingFiles'

// What the API's refusals say, by their codes; anything else is its own message.
const refusalKeyByCode = new Map<string, ParseKeys<'meetings'>>([
  [ 'meeting_upload_not_media', 'upload.refusals.notMedia' ],
  [ 'meeting_upload_not_text', 'upload.refusals.notText' ],
  [ 'meeting_upload_nothing_said', 'upload.refusals.nothingSaid' ],
  [ 'meeting_upload_too_many_lines', 'upload.refusals.tooManyLines' ],
  [ 'meeting_upload_location', 'upload.refusals.location' ],
  [ 'meeting_processing', 'upload.refusals.processing' ],
  [ 'meeting_run_failed', 'upload.refusals.runFailed' ],
  [ 'meeting_merged', 'upload.refusals.merged' ],
])

// A transcript the API could not read: the upload cannot finish with it, so it is let go, and
// the person chooses another.
const TRANSCRIPT_REFUSALS = new Set([
  'meeting_upload_not_text',
  'meeting_upload_nothing_said',
  'meeting_upload_too_many_lines',
])

type Props = {
  // The meeting a capture is added to, or null for a new meeting.
  meeting: Meeting | null
  policy: MeetingUploadPolicy
  // The person's unfinished uploads, one of which the same files may resume.
  unfinished: MeetingUpload[]
}

// The files of an upload, and for a new meeting its title and when it started. Upload is the
// page's one save: it starts the upload, or resumes the one these same files began, sends every
// part it lacks with its progress beside the fields, and once it is whole makes the meeting and
// opens it. Leaving while it runs asks first, as leaving unsaved work does, and stops it where it
// is, for the same files to resume.
export function UploadMeetingForm(props: Props) {
  const { t } = useTranslation([ 'meetings', 'common' ])
  const dispatch = useAppDispatch()
  const navigate = useNavigate()
  const storageLocations = useAppSelector(selectAllStorageLocations)
  const [ files, setFiles ] = useState<ChosenFiles>({ recording: null, transcript: null })
  const [ fileErrors, setFileErrors ] = useState<Partial<Record<MeetingUploadFileKind, string>>>({})
  const [ progress, setProgress ] = useState<{ received: number, total: number } | null>(null)
  const [ isFinishing, setIsFinishing ] = useState(false)
  const sendingRef = useRef<AbortController | null>(null)
  // The uploads this visit started, which the list read as the page opened does not hold yet, so
  // Upload after one stopped carries it on rather than starting another.
  const startedRef = useRef<MeetingUpload[]>([])
  const meetingId = props.meeting?.id ?? null
  const isNewMeeting = meetingId === null
  const hasRecording = files.recording !== null
  const hasFile = MEETING_UPLOAD_FILE_KINDS.some((kind) => files[kind] !== null)
  const isSending = progress !== null
  // A new meeting starts at its title; a capture at choosing its recording.
  const titleRef = useInitialFocus(isNewMeeting)
  const recordingRef = useInitialFocus(!isNewMeeting)

  // Leaving the page stops what it is sending.
  useEffect(() => () => sendingRef.current?.abort(), [])

  const resolver = useMemo(
    () => zodResolver(createMeetingUploadFormSchema(t, { isNewMeeting, hasRecording })),
    [ t, isNewMeeting, hasRecording ],
  )
  const form = useForm<MeetingUploadFormValues>({
    resolver,
    defaultValues: {
      title: '',
      startedAt: null,
      storageLocationId: props.policy.defaultStorageLocationId ?? '',
    },
  })
  // A new meeting's title is kept for the tab until it uploads; files are not, which storage
  // cannot hold.
  const forgetDraft = useFormDraft(form, { name: 'meeting-upload', fields: [ 'title' ], isEnabled: isNewMeeting })

  function chooseFile(kind: MeetingUploadFileKind, file: File | null) {
    setFiles((chosen) => ({ ...chosen, [kind]: file }))
    setFileErrors((errors) => ({ ...errors, [kind]: undefined }))
    if (!file || !isNewMeeting) {
      return
    }
    // A new meeting is named and timed from its first file until the person says otherwise:
    // a recorder names its file for the meeting and writes it as the meeting ends.
    const values = form.getValues()
    if (!values.title.trim()) {
      form.setValue('title', titleFromFileName(file.name), { shouldDirty: true, shouldValidate: true })
    }
    if (!values.startedAt) {
      const changedAt = new Date(file.lastModified).toISOString()
      form.setValue('startedAt', changedAt, { shouldDirty: true, shouldValidate: true })
    }
  }

  // What the upload is of, as the request that starts it says it and an earlier one is matched.
  function detailsOf(values: MeetingUploadFormValues): UploadDetails {
    return {
      meetingId,
      title: isNewMeeting
        ? values.title.trim()
        : null,
      startedAt: isNewMeeting
        ? values.startedAt
        : null,
      storageLocationId: hasRecording
        ? values.storageLocationId
        : null,
    }
  }

  function requestOf(details: UploadDetails): CreateMeetingUploadRequest {
    const request: CreateMeetingUploadRequest = {}
    if (details.meetingId) {
      request.meetingId = details.meetingId
    }
    if (details.title) {
      request.title = details.title
    }
    if (details.startedAt) {
      request.startedAt = details.startedAt
    }
    if (details.storageLocationId) {
      request.storageLocationId = details.storageLocationId
    }
    for (const kind of MEETING_UPLOAD_FILE_KINDS) {
      const file = files[kind]
      if (file) {
        request[kind] = {
          name: file.name,
          sizeBytes: file.size,
          modifiedAt: new Date(file.lastModified).toISOString(),
        }
      }
    }
    return request
  }

  // Says why the upload stopped. A transcript the API could not read lets the upload go and
  // marks the field, for another to be chosen; everything else keeps it, for Upload to resume.
  async function reportRefusal(error: unknown, upload: MeetingUpload | null) {
    const code = getApiErrorCode(error)
    const key = code
      ? refusalKeyByCode.get(code)
      : undefined
    const message = key
      ? t(key)
      : getApiErrorMessage(error) ?? t('upload.toasts.stopped')
    if (code && TRANSCRIPT_REFUSALS.has(code)) {
      setFileErrors((errors) => ({ ...errors, transcript: message }))
      if (upload) {
        const refused = upload.id
        startedRef.current = startedRef.current.filter((started) => started.id !== refused)
        await cancelMeetingUpload(refused).catch((cancelError: unknown) => {
          console.debug('UploadMeetingForm could not let a refused upload go', { cancelError, uploadId: refused })
        })
      }
      return
    }
    console.debug('UploadMeetingForm stopped an upload', { error, uploadId: upload?.id })
    toast.danger(message)
  }

  // The upload these same files began, this visit or an earlier one, as the API has it now, so it
  // carries on from the first part it lacks; null when there is none to carry on, or it was let go
  // meanwhile.
  async function resume(details: UploadDetails) {
    const found = findResumable([ ...startedRef.current, ...props.unfinished ], details, files)
    if (!found) {
      return null
    }
    try {
      const { upload } = await getMeetingUpload(found.id)
      return upload.finishedAt
        ? null
        : upload
    }
    catch (error) {
      console.debug('UploadMeetingForm found no upload to carry on, and starts another', { error, uploadId: found.id })
      return null
    }
  }

  const save = saveForm(form, async (values) => {
    const sending = new AbortController()
    sendingRef.current = sending
    let upload: MeetingUpload | null = null
    try {
      const details = detailsOf(values)
      upload = await resume(details)
      if (upload) {
        toast(t('upload.toasts.resumed'))
      }
      else {
        upload = (await createMeetingUpload(requestOf(details))).upload
        startedRef.current = [ upload, ...startedRef.current ]
      }
      setProgress(uploadProgress(upload))
      upload = await sendMeetingFiles({
        upload,
        files,
        signal: sending.signal,
        onProgress: (moved) => setProgress(uploadProgress(moved)),
      })
      setIsFinishing(true)
      const response = await finishMeetingUpload(upload.id)
      dispatch(meetingUpserted(response.meeting))
      forgetDraft()
      toast.success(t('upload.toasts.uploaded', { title: response.meeting.title || t('list.untitled') }))
      void navigate(getMeetingUrl(response.meeting.id))
      return true
    }
    catch (error) {
      if (sending.signal.aborted) {
        console.debug('UploadMeetingForm stopped sending as the page closed', { uploadId: upload?.id })
        return false
      }
      await reportRefusal(error, upload)
      return false
    }
    finally {
      sendingRef.current = null
      setProgress(null)
      setIsFinishing(false)
    }
  })

  const onSubmit = usePageSave({
    save,
    onDiscard: forgetDraft,
    isDirty: form.formState.isDirty || hasFile,
    blockedReason: hasFile
      ? null
      : t('upload.blocked.noFile'),
    label: t('upload.submit'),
  })

  const errors = form.formState.errors
  const [ title, startedAt, storageLocationId ] = useWatch({
    control: form.control,
    name: [ 'title', 'startedAt', 'storageLocationId' ],
  })
  const locationOptions = storageLocations.map((location) => ({ id: location.id, label: location.name }))

  return <Form onSubmit={onSubmit} validationBehavior='aria' className='flex max-w-3xl flex-col gap-6'>
    <div className='grid grid-cols-1 gap-4 md:grid-cols-2'>
      {MEETING_UPLOAD_FILE_KINDS.map((kind) => <MeetingFileField
        key={kind}
        kind={kind}
        policy={props.policy}
        file={files[kind]}
        isDisabled={isSending}
        errorMessage={fileErrors[kind]}
        chooseRef={kind === 'recording'
          ? recordingRef
          : undefined}
        onChange={(file) => chooseFile(kind, file)}
      />)}
    </div>

    {isNewMeeting && <div className='grid grid-cols-1 gap-4 md:grid-cols-2'>
      <TextField
        id='meeting-upload-title'
        isRequired
        autoComplete='off'
        isDisabled={isSending}
        isInvalid={Boolean(errors.title)}
        value={title}
        onChange={(value) => form.setValue('title', value, { shouldDirty: true, shouldValidate: true })}
      >
        <Label>{t('upload.fields.title')}</Label>
        <Input ref={titleRef} />
        <FieldError>{errors.title?.message}</FieldError>
      </TextField>
      <MeetingStartField
        value={startedAt}
        isDisabled={isSending}
        errorMessage={errors.startedAt?.message}
        onChange={(value) => form.setValue('startedAt', value, { shouldDirty: true, shouldValidate: true })}
      />
    </div>}

    {hasRecording && <div className='flex max-w-md flex-col gap-1'>
      {locationOptions.length
        ? <OptionSelect
          id='meeting-upload-storage-location'
          label={t('upload.fields.storageLocation')}
          placeholder={t('upload.fields.storageLocationPlaceholder')}
          options={locationOptions}
          value={storageLocationId}
          isDisabled={isSending}
          onChange={(value) => form.setValue('storageLocationId', value, { shouldDirty: true, shouldValidate: true })}
        />
        : <p className='text-sm opacity-70'>
          {t('upload.fields.noStorageLocations')}{' '}
          <Link href={UrlTree.settingsStorageNew} className='text-link'>{t('upload.fields.addStorageLocation')}</Link>
        </p>}
      {errors.storageLocationId && <p className='text-xs text-danger' role='alert'>{
        errors.storageLocationId.message
      }</p>}
    </div>}

    {progress && <ProgressBar
      aria-label={t('upload.progress.label')}
      value={progress.total
        ? Math.round(progress.received / progress.total * 100)
        : 0}
      className='w-full'
    >
      <Label>{isFinishing
        ? t('upload.progress.finishing')
        : t('upload.progress.sending', {
          received: formatBytes(progress.received),
          total: formatBytes(progress.total),
        })}</Label>
      <ProgressBar.Output />
      <ProgressBar.Track>
        <ProgressBar.Fill />
      </ProgressBar.Track>
    </ProgressBar>}

    <PageCreateFooter />
  </Form>
}
