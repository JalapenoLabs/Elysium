// Copyright © 2026 Jalapeno Labs

// Misc
import { API_BASE_PATH } from '../../constants'
import { apiClient } from '../index'

// Mirrors `ProjectResponse` in api/src/routes/v1/projects/mod.rs.
export type Project = {
  id: string
  name: string
  description: string
  createdAt: string
  updatedAt: string
  // When the cover last changed; null when the project has none.
  coverUpdatedAt: string | null
}

type ListProjectsResponse = {
  projects: Project[]
}

export function listProjects() {
  return apiClient
    .get('v1/projects')
    .json<ListProjectsResponse>()
}

type CreateProjectRequest = {
  name: string
  description?: string
}

type CreateProjectResponse = {
  project: Project
}

export function createProject(body: CreateProjectRequest) {
  return apiClient
    .post('v1/projects', { json: body })
    .json<CreateProjectResponse>()
}

// Omitted fields are left unchanged.
type UpdateProjectRequest = {
  name?: string
  description?: string
}

type UpdateProjectResponse = {
  project: Project
}

export function updateProject(projectId: string, body: UpdateProjectRequest) {
  return apiClient
    .patch(`v1/projects/${projectId}`, { json: body })
    .json<UpdateProjectResponse>()
}

type ProjectCoverResponse = {
  project: Project
}

// Sends the file itself as the body. The API compresses it and answers 400 for anything
// that is not a PNG, JPEG, WebP, or GIF image under 1 MB.
export function uploadProjectCover(projectId: string, file: File) {
  return apiClient
    .put(`v1/projects/${projectId}/cover`, { body: file })
    .json<ProjectCoverResponse>()
}

export function deleteProjectCover(projectId: string) {
  return apiClient
    .delete(`v1/projects/${projectId}/cover`)
    .json<ProjectCoverResponse>()
}

// Not a fetch: an <img> source. The version in the query string gives each cover its own
// URL, which the API lets browsers cache for good.
export function getProjectCoverUrl(project: Project) {
  if (!project.coverUpdatedAt) {
    return null
  }
  return `${API_BASE_PATH}/v1/projects/${project.id}/cover?v=${encodeURIComponent(project.coverUpdatedAt)}`
}

// Answers 409 while any coding session still belongs to the project.
export function deleteProject(projectId: string) {
  return apiClient
    .delete(`v1/projects/${projectId}`)
}
