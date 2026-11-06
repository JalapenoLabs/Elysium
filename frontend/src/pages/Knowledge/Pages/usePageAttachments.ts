// Copyright © 2026 Jalapeno Labs

import type { EditorAttachments } from '../../../components/markdownEditor/MarkdownEditorContent'
import type { PageAttachment } from '../../../api/routes/pageRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { pageLoaded } from '../../../store/pagesSlice'

// Misc
import { attachPageFile, getPageAttachmentPolicy, PAGE_ATTACHMENT_POLICY_KEY } from '../../../api/routes/pageRoutes'
import { toast } from '../../../toast/toast'
import { attachmentAccept, describeFileRefusal, describeUploadRefusal } from './pageAttachmentPresentation'

// What a page takes and where it goes. `pageAttachments.changed` loads it again, so an admin's
// choice reaches everyone looking at a page.
export function usePageAttachmentPolicy() {
  const { data, error } = useSWR(PAGE_ATTACHMENT_POLICY_KEY, getPageAttachmentPolicy)

  return {
    policy: data?.policy ?? null,
    hasFailed: Boolean(error),
  } as const
}

// Attaches files to page `pageId`, one call a file, and says what went wrong in a toast of its own
// for each one that did: a file refused never holds up the ones beside it. `uploading` names what
// is on its way.
export function usePageAttachmentUpload(pageId: string) {
  const { t } = useTranslation('knowledge')
  const dispatch = useAppDispatch()
  const { policy, hasFailed } = usePageAttachmentPolicy()
  const [ uploading, setUploading ] = useState<string[]>([])

  async function attach(file: File): Promise<PageAttachment | null> {
    if (!policy) {
      console.debug('usePageAttachmentUpload has no policy to check a file against', { name: file.name })
      toast.danger(t('attachments.refused', { name: file.name }), {
        description: t('attachments.unavailable'),
      })
      return null
    }

    const refusal = describeFileRefusal(file, policy, t)
    if (refusal) {
      console.debug('usePageAttachmentUpload refused a file before sending it', { name: file.name, refusal })
      toast.danger(t('attachments.refused', { name: file.name }), { description: refusal })
      return null
    }

    setUploading((names) => [ ...names, file.name ])
    try {
      const response = await attachPageFile(pageId, file)
      dispatch(pageLoaded(response.page))
      return response.attachment
    }
    catch (error) {
      console.debug('usePageAttachmentUpload could not attach a file', { error, pageId, name: file.name })
      toast.danger(t('attachments.refused', { name: file.name }), {
        description: describeUploadRefusal(error, policy, t),
      })
      return null
    }
    finally {
      setUploading((names) => {
        const index = names.indexOf(file.name)
        return names.toSpliced(index, 1)
      })
    }
  }

  const accept = policy
    ? attachmentAccept(policy)
    : ''
  // Nothing can be attached until an admin chooses where it goes, so everyone is told first.
  const hasNoLocation = policy !== null && policy.destinationId === null

  let editorAttachments: EditorAttachments = { canAttach: true, accept, attach }
  if (!policy) {
    editorAttachments = { canAttach: false, reason: t('attachments.unavailable') }
  }
  else if (hasNoLocation) {
    editorAttachments = { canAttach: false, reason: t('attachments.noLocation') }
  }

  return {
    attach,
    policy,
    hasFailed,
    uploading,
    accept,
    hasNoLocation,
    // The same, for the rich editor, which puts a reference to each file into the text.
    editorAttachments,
  } as const
}
