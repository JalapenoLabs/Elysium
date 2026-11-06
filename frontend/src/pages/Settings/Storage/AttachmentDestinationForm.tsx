// Copyright © 2026 Jalapeno Labs

import type { StorageLocation } from '../../../api/routes/storageRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { mutate } from 'swr'

// User interface
import { Form, Link } from '@heroui/react'
import { OptionSelect } from '../../../components/OptionSelect'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { choosePageAttachmentDestination, PAGE_ATTACHMENT_POLICY_KEY } from '../../../api/routes/pageRoutes'
import { usePageSave } from '../../../layout/page/pageSave'
import { toast } from '../../../toast/toast'
import { UrlTree } from '../../../urls'

// The select's id for keeping attachments nowhere. Location ids are UUIDs, so it never collides.
const NOWHERE = 'nowhere'

type Props = {
  // The location attachments go to now, or null for none.
  destinationId: string | null
  locations: StorageLocation[]
  // Once saved, as an edit page goes back to its list.
  onSaved: () => void
}

// Where new page attachments go, saved from the page's Save in the header. The API reaches the
// location before it is chosen, so a wrong key is reported here rather than to whoever attaches the
// next file. Files already attached stay where they landed.
export function AttachmentDestinationForm(props: Props) {
  const { t } = useTranslation([ 'storage', 'common' ])
  const saved = props.destinationId ?? NOWHERE
  const [ chosen, setChosen ] = useState(saved)

  async function save() {
    try {
      const response = await choosePageAttachmentDestination(chosen === NOWHERE
        ? null
        : chosen)
      await mutate(PAGE_ATTACHMENT_POLICY_KEY, response, { revalidate: false })
      const location = props.locations.find((candidate) => candidate.id === response.policy.destinationId)
      toast.success(location
        ? t('attachments.saved', { name: location.name })
        : t('attachments.turnedOff'))
      props.onSaved()
      return true
    }
    catch (error) {
      const message = getApiErrorMessage(error)
      if (!message) {
        console.debug('AttachmentDestinationForm could not choose a destination', { error, chosen })
      }
      toast.danger(t('attachments.failed'), {
        description: message ?? t('common:errors.unexpected'),
      })
      return false
    }
  }

  const onSubmit = usePageSave({
    save,
    isDirty: chosen !== saved,
  })

  const options = [
    { id: NOWHERE, label: t('attachments.none') },
    ...props.locations.map((location) => ({ id: location.id, label: location.name })),
  ]

  return <Form
    className='relaxed max-w-md'
    onSubmit={onSubmit}
  >
    <OptionSelect
      id='attachment-destination'
      label={t('attachments.label')}
      options={options}
      value={chosen}
      onChange={setChosen}
    />
    {!props.locations.length && <Link
      className='mt-2 text-sm'
      href={UrlTree.settingsStorageNew}
    >{
        t('attachments.addLocation')
      }</Link>}
  </Form>
}
