// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// A category learnings are filed under: one of Elysium's nine built-in ones, or one of the
// workspace's own. Mirrors `LearningCategoryResponse` in
// api/src/routes/v1/learning_categories/mod.rs; docs/knowledge.md, Categories.
export type LearningCategory = {
  // What a learning stores and an agent sends. Never changes.
  key: string
  label: string
  // What belongs there, written for the agent choosing a category.
  description: string
  // Its place among the categories of its kind; the list arrives in order.
  position: number
  // One of Elysium's nine, which cannot be changed or removed.
  builtIn: boolean
  // The learnings filed under it, deleted ones included: what removing it moves.
  learningCount: number
  createdBy: string
  createdAt: string
  updatedAt: string
}

// The list's key, revalidated by `learningCategories.changed` and by `learning.changed`, which
// can change a category's count.
export const LEARNING_CATEGORIES_KEY = 'v1/learning-categories'

type LearningCategoriesResponse = {
  categories: LearningCategory[]
}

export function listLearningCategories() {
  return apiClient
    .get(LEARNING_CATEGORIES_KEY)
    .json<LearningCategoriesResponse>()
}

type LearningCategoryResponse = {
  category: LearningCategory
}

export type CreateLearningCategoryRequest = {
  key: string
  label: string
  description: string
}

export function createLearningCategory(body: CreateLearningCategoryRequest) {
  return apiClient
    .post(LEARNING_CATEGORIES_KEY, { json: body })
    .json<LearningCategoryResponse>()
}

// Omitted fields keep their value. `position` is its place among the workspace's own, from 0.
export type UpdateLearningCategoryRequest = {
  label?: string
  description?: string
  position?: number
}

export function updateLearningCategory(key: string, body: UpdateLearningCategoryRequest) {
  return apiClient
    .patch(`${LEARNING_CATEGORIES_KEY}/${key}`, { json: body })
    .json<LearningCategoryResponse>()
}

// A category with learnings needs `moveTo`, the category they move to; without it the API
// answers `409 category_in_use`.
export function removeLearningCategory(key: string, moveTo: string | null) {
  const searchParams = moveTo
    ? { moveTo }
    : undefined
  return apiClient.delete(`${LEARNING_CATEGORIES_KEY}/${key}`, { searchParams })
}
