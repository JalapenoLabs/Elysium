// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { Finding, FindingCost } from '../../api/routes/findingRoutes'

// How a project's findings read on its Findings tab. Pure, and tested beside. See
// docs/autonomy.md, Findings.

type ChipColor = 'default' | 'accent' | 'success' | 'warning' | 'danger'

export const findingCostLabelKeys = {
  small: 'findings.costs.small',
  medium: 'findings.costs.medium',
  large: 'findings.costs.large',
} as const satisfies Record<FindingCost, ParseKeys<'projects'>>

// The larger the work, the more a person should weigh it before accepting.
export const findingCostChipColors = {
  small: 'success',
  medium: 'default',
  large: 'warning',
} as const satisfies Record<FindingCost, ChipColor>

// Smallest first, so sorting by cost reads small, medium, large rather than alphabetically.
export const findingCostOrder = {
  small: 0,
  medium: 1,
  large: 2,
} as const satisfies Record<FindingCost, number>

// The address a finding's repository opens at in a browser, or null when it has none or names
// one only Git reads, such as an SSH remote (`git@github.com:org/repo`).
export function repositoryHref(finding: Pick<Finding, 'repositoryUrl'>): string | null {
  const url = finding.repositoryUrl
  if (!url) {
    return null
  }
  if (!url.startsWith('https://') && !url.startsWith('http://')) {
    return null
  }
  return url.replace(/\.git$/, '')
}
