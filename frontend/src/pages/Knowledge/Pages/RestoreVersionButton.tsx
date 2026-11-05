// Copyright © 2026 Jalapeno Labs

// Core
import { useContext, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useSWRConfig } from 'swr'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { pageLoaded } from '../../../store/pagesSlice'

// User interface
import { Button, Tooltip } from '@heroui/react'
import { LuHistory } from 'react-icons/lu'

// Misc
import { getApiErrorCode, getApiErrorMessage } from '../../../api/errors'
import { pageHistoryKey, restorePageVersion } from '../../../api/routes/pageRoutes'
import { PageSaveStateContext } from '../../../layout/page/pageSave'
import { toast } from '../../../toast/toast'

type Props = {
  pageId: string
  version: number
}

// Refusals a person can act on, in the app's words; anything else is shown as the API words it.
const refusalKeys = {
  page_title_taken: 'pages.history.toasts.titleTaken',
  page_deleted: 'pages.history.toasts.deleted',
} as const

function isKnownRefusal(code: string | null): code is keyof typeof refusalKeys {
  return code !== null && code in refusalKeys
}

// Puts a version back. It acts at once rather than asking, since nothing is lost: the version it
// replaces stays, and the toast's Undo restores that one in turn, from wherever the person is by then.
export function RestoreVersionButton(props: Props) {
  const { t } = useTranslation([ 'knowledge', 'common' ])
  const dispatch = useAppDispatch()
  const { mutate } = useSWRConfig()
  const pageSave = useContext(PageSaveStateContext)
  const [ isRestoring, setIsRestoring ] = useState(false)
  const { pageId, version } = props
  const label = t('pages.history.restoreLabel', { version })

  async function restore(restoredVersion: number) {
    try {
      const { page, outcome } = await restorePageVersion(pageId, restoredVersion)
      dispatch(pageLoaded(page))
      void mutate(pageHistoryKey(pageId))

      if (outcome === 'unchanged') {
        toast(t('pages.history.toasts.alreadyCurrent', { version: restoredVersion }))
        return
      }

      // A restore is always the next version, so the one before it is what it replaced, even if
      // someone published after this view loaded.
      const replacedVersion = page.currentVersion - 1
      toast.success(
        t('pages.history.toasts.restored', { version: restoredVersion, newVersion: page.currentVersion }),
        { action: { label: t('common:actions.undo'), onPress: () => void restore(replacedVersion) }},
      )
    }
    catch (error) {
      console.debug('RestoreVersionButton could not restore a version', { error, pageId, restoredVersion })
      const code = getApiErrorCode(error)
      if (isKnownRefusal(code)) {
        toast.warning(t(refusalKeys[code]))
        return
      }
      toast.danger(t('common:errors.unexpected'), { description: getApiErrorMessage(error) })
    }
  }

  async function press() {
    setIsRestoring(true)
    await restore(version)
    setIsRestoring(false)
  }

  // A restore would change the page under what is being edited on it.
  if (pageSave?.save?.isDirty) {
    return <Tooltip delay={300}>
      {/* A disabled button fires no hover, so its tooltip hangs on a wrapper. */}
      <Tooltip.Trigger>
        <div>
          <Button size='sm' variant='ghost' aria-label={label} isDisabled>
            <LuHistory className='size-4' aria-hidden />
            <span>{t('pages.history.restore')}</span>
          </Button>
        </div>
      </Tooltip.Trigger>
      <Tooltip.Content>
        <span>{t('pages.history.saveFirst')}</span>
      </Tooltip.Content>
    </Tooltip>
  }

  return <Button size='sm' variant='ghost' aria-label={label} isPending={isRestoring} onPress={press}>
    <LuHistory className='size-4' aria-hidden />
    <span>{t('pages.history.restore')}</span>
  </Button>
}
