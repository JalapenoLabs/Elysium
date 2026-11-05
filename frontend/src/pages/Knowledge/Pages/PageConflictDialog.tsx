// Copyright © 2026 Jalapeno Labs

import type { KnowledgePage, PageVersion } from '../../../api/routes/pageRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Modal, Tabs } from '@heroui/react'
import { TextDiff } from '../../../components/TextDiff'
import { PageByline } from './PageByline'

// Misc
import { pageAsDiffText } from './pageForm'
import { useInitialFocus } from '../../../hooks/useInitialFocus'

export type PageConflict = {
  // The version the person was editing from, and what the page says now.
  opened: PageVersion
  latest: KnowledgePage
}

type Props = {
  conflict: PageConflict
  // What the person has written, which nothing here touches.
  draft: { title: string, body: string, evidence?: string | null }
  isOpen: boolean
  onKeepEditing: () => void
}

// Told when a publish is refused because someone published first: who did, and what they changed
// since this person opened the page, or how it stands against their text. Their text stays
// untouched. Having seen the newer version, publishing again replaces it, so nobody overwrites what
// they have not read.
export function PageConflictDialog(props: Props) {
  const { t } = useTranslation('knowledge')
  const { opened, latest } = props.conflict
  const keepEditingRef = useInitialFocus()
  const latestText = pageAsDiffText(latest)
  const foldedLabel = (count: number) => t('pages.conflict.foldedLines', { count })

  return <Modal.Backdrop
    isOpen={props.isOpen}
    onOpenChange={(isOpen) => {
      if (!isOpen) {
        props.onKeepEditing()
      }
    }}
  >
    <Modal.Container scroll='inside'>
      <Modal.Dialog className='sm:max-w-3xl'>
        <Modal.Header>
          <Modal.Heading>{t('pages.conflict.title')}</Modal.Heading>
        </Modal.Header>
        <Modal.Body className='flex flex-col gap-3'>
          <PageByline page={latest} />
          <Tabs>
            <Tabs.ListContainer>
              <Tabs.List aria-label={t('pages.conflict.viewsLabel')}>
                <Tabs.Tab id='theirs'>
                  <span>{t('pages.conflict.theirChanges')}</span>
                  <Tabs.Indicator />
                </Tabs.Tab>
                <Tabs.Tab id='yours'>
                  <span>{t('pages.conflict.againstYours')}</span>
                  <Tabs.Indicator />
                </Tabs.Tab>
              </Tabs.List>
            </Tabs.ListContainer>
            <Tabs.Panel id='theirs' className='pt-3'>
              <TextDiff
                before={pageAsDiffText(opened)}
                after={latestText}
                label={t('pages.conflict.theirChanges')}
                foldedLabel={foldedLabel}
              />
            </Tabs.Panel>
            <Tabs.Panel id='yours' className='pt-3'>
              <TextDiff
                before={latestText}
                after={pageAsDiffText(props.draft)}
                label={t('pages.conflict.againstYours')}
                foldedLabel={foldedLabel}
              />
            </Tabs.Panel>
          </Tabs>
          <p className='text-sm'>{t('pages.conflict.publishAgain')}</p>
        </Modal.Body>
        <Modal.Footer>
          <Button ref={keepEditingRef} onPress={props.onKeepEditing}>
            <span>{t('pages.conflict.keepEditing')}</span>
          </Button>
        </Modal.Footer>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}
