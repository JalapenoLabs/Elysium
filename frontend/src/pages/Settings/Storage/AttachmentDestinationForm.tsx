// Copyright © 2026 Jalapeno Labs

import type { StorageLocation } from '../../../api/routes/storageRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { mutate } from 'swr'

// User interface
import { Description, Form, Label, Link, NumberField } from '@heroui/react'
import { OptionSelect } from '../../../components/OptionSelect'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { MEETING_ATTACHMENT_POLICY_KEY, setMeetingAttachmentLimit } from '../../../api/routes/meetingAttachmentRoutes'
import { choosePageAttachmentDestination, PAGE_ATTACHMENT_POLICY_KEY } from '../../../api/routes/pageRoutes'
import { formatBytes } from '../../../format/numbers'
import { usePageSave } from '../../../layout/page/pageSave'
import { toast } from '../../../toast/toast'
import { UrlTree } from '../../../urls'

// The select's id for keeping attachments nowhere. Location ids are UUIDs, so it never collides.
const NOWHERE = 'nowhere'

// A meeting's largest file is chosen in gigabytes, as storage providers count them, to the
// megabyte.
const BYTES_PER_GIGABYTE = 1e9
const SMALLEST_LIMIT_GIGABYTES = 0.001

type Props = {
  // The location attachments go to now, or null for none.
  destinationId: string | null
  // The largest file a meeting takes now, and the most it may be set to.
  meetingMaxBytes: number
  meetingMaxBytesCeiling: number
  locations: StorageLocation[]
  // Once saved, as an edit page goes back to its list.
  onSaved: () => void
}

// Where new page and meeting attachments go, and the largest file a meeting takes, saved from the
// page's Save in the header. The API reaches the location before it is chosen, so a wrong key is
// reported here rather than to whoever attaches the next file. Files already attached stay where
// they landed.
export function AttachmentDestinationForm(props: Props) {
  const { t } = useTranslation([ 'storage', 'common' ])
  const saved = props.destinationId ?? NOWHERE
  const savedGigabytes = props.meetingMaxBytes / BYTES_PER_GIGABYTE
  const [ chosen, setChosen ] = useState(saved)
  // NaN while the field is empty.
  const [ gigabytes, setGigabytes ] = useState(savedGigabytes)
  const isDestinationDirty = chosen !== saved
  const isLimitDirty = gigabytes !== savedGigabytes

  async function saveDestination() {
    const response = await choosePageAttachmentDestination(chosen === NOWHERE
      ? null
      : chosen)
    await mutate(PAGE_ATTACHMENT_POLICY_KEY, response, { revalidate: false })
    const location = props.locations.find((candidate) => candidate.id === response.policy.destinationId)
    toast.success(location
      ? t('attachments.saved', { name: location.name })
      : t('attachments.turnedOff'))
  }

  async function saveLimit() {
    const response = await setMeetingAttachmentLimit(Math.round(gigabytes * BYTES_PER_GIGABYTE))
    await mutate(MEETING_ATTACHMENT_POLICY_KEY, response, { revalidate: false })
    toast.success(t('attachments.meetingLimitSaved', { size: formatBytes(response.policy.maxBytes) }))
  }

  // The location first, since the API reaches it and may refuse it; the limit always saves.
  async function save() {
    try {
      if (isDestinationDirty) {
        await saveDestination()
      }
      if (isLimitDirty) {
        await saveLimit()
      }
      props.onSaved()
      return true
    }
    catch (error) {
      const message = getApiErrorMessage(error)
      if (!message) {
        console.debug('AttachmentDestinationForm could not save', { error, chosen, gigabytes })
      }
      toast.danger(t('attachments.failed'), {
        description: message ?? t('common:errors.unexpected'),
      })
      return false
    }
  }

  const onSubmit = usePageSave({
    save,
    isDirty: isDestinationDirty || isLimitDirty,
    blockedReason: Number.isNaN(gigabytes)
      ? t('attachments.meetingLimitRequired')
      : null,
  })

  const options = [
    { id: NOWHERE, label: t('attachments.none') },
    ...props.locations.map((location) => ({ id: location.id, label: location.name })),
  ]

  return <Form
    className='relaxed flex max-w-md flex-col gap-6'
    onSubmit={onSubmit}
  >
    <div className='flex flex-col gap-2'>
      <OptionSelect
        id='attachment-destination'
        label={t('attachments.label')}
        options={options}
        value={chosen}
        onChange={setChosen}
      />
      <p className='text-sm opacity-70'>{t('attachments.description')}</p>
      {!props.locations.length && <Link
        className='text-sm'
        href={UrlTree.settingsStorageNew}
      >{
          t('attachments.addLocation')
        }</Link>}
    </div>
    <NumberField
      id='meeting-attachment-limit'
      value={gigabytes}
      onChange={setGigabytes}
      minValue={SMALLEST_LIMIT_GIGABYTES}
      maxValue={props.meetingMaxBytesCeiling / BYTES_PER_GIGABYTE}
      step={0.1}
      formatOptions={{ style: 'unit', unit: 'gigabyte', maximumFractionDigits: 3 }}
    >
      <Label>{t('attachments.meetingLimit')}</Label>
      <NumberField.Group>
        <NumberField.DecrementButton />
        <NumberField.Input />
        <NumberField.IncrementButton />
      </NumberField.Group>
      <Description>{
        t('attachments.meetingLimitHint', { ceiling: formatBytes(props.meetingMaxBytesCeiling) })
      }</Description>
    </NumberField>
  </Form>
}
