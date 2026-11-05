// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Label, TextArea, TextField } from '@heroui/react'
import { OptionSelect } from '../../../components/OptionSelect'

// Misc
import { useLearningCategories } from '../../../hooks/useLearningCategories'

type Props = {
  category: string
  evidence: string
  onCategoryChange: (category: string) => void
  onEvidenceChange: (evidence: string) => void
}

// What a learning holds beyond its text, edited beside it. A category removed since stays in the
// list as its key, so opening a learning never changes where it is filed.
export function LearningEditFields(props: Props) {
  const { t } = useTranslation('knowledge')
  const { status, categories, labels } = useLearningCategories()

  const options = categories.map((category) => ({ id: category.key, label: category.label }))
  if (props.category && !(props.category in labels)) {
    options.push({ id: props.category, label: props.category })
  }

  return <>
    <TextField id='page-evidence' value={props.evidence} onChange={props.onEvidenceChange}>
      <Label>{t('pages.evidence')}</Label>
      <TextArea rows={4} autoComplete='off' />
    </TextField>
    <OptionSelect
      id='page-category'
      label={t('pages.editor.category')}
      options={options}
      value={props.category}
      isDisabled={status !== 'loaded'}
      onChange={props.onCategoryChange}
    />
  </>
}
