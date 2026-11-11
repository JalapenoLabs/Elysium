// Copyright © 2026 Jalapeno Labs

// Misc
import { API_BASE_PATH, PROJECT_COVER_UPLOAD_TIMEOUT_MS } from '../../constants'
import { apiClient } from '../index'

// Mirrors `ProjectScope` in api/src/models/project.rs: every project, including ones
// added later, or only the listed project ids.
export const ALL_PROJECTS = '*'
export type ProjectScope = typeof ALL_PROJECTS | string[]

// Mirrors `ProjectCoverFit` in api/src/models/project.rs. `fit` shows the whole image over a
// blurred copy of itself; `fill` covers the frame and crops what does not fit.
export const PROJECT_COVER_FITS = [ 'fit', 'fill' ] as const
export type ProjectCoverFit = typeof PROJECT_COVER_FITS[number]

// Mirrors `GithubAccess` in api/src/models/project.rs: how a project picks the GitHub token
// its sessions start with.
export const GITHUB_ACCESS_CHOICES = [ 'default', 'none', 'specific' ] as const
export type GithubAccess = typeof GITHUB_ACCESS_CHOICES[number]

// Mirrors `ProjectGithub` in api/src/routes/v1/projects/mod.rs. `credentialId` is set only for
// `specific` access; `specific` with a null id means the chosen token was deleted, and the
// project follows the workspace default until it chooses again.
export type ProjectGithub = {
  access: GithubAccess
  credentialId: string | null
}

// Mirrors `Autonomy` in api/src/models/project_autonomy.rs: a project's trust dial, what of its
// Project Manager's work waits for a person. Found work waits at every level.
export const PROJECT_AUTONOMY_LEVELS = [
  'approve_every_batch',
  'approve_found_work',
  'full_autonomy',
] as const
export type ProjectAutonomy = typeof PROJECT_AUTONOMY_LEVELS[number]

// Mirrors `ProjectResponse` in api/src/routes/v1/projects/mod.rs.
export type Project = {
  id: string
  name: string
  description: string
  createdAt: string
  // The user who created it: a person, or a machine such as the coding agent.
  createdBy: string
  updatedAt: string
  // When the cover last changed; null when the project has none.
  coverUpdatedAt: string | null
  coverFit: ProjectCoverFit
  github: ProjectGithub
  // Who set it and when is `getProjectAutonomy`.
  autonomy: ProjectAutonomy
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
export type UpdateProjectRequest = {
  name?: string
  description?: string
  coverFit?: ProjectCoverFit
  github?: ProjectGithub
}

type UpdateProjectResponse = {
  project: Project
}

export function updateProject(projectId: string, body: UpdateProjectRequest) {
  return apiClient
    .patch(`v1/projects/${projectId}`, { json: body })
    .json<UpdateProjectResponse>()
}

// Mirrors `AutonomyResponse` in api/src/routes/v1/projects/autonomy.rs. `setBy` and `setAt` are
// null while the project is at the default it started with.
export type ProjectAutonomySetting = {
  autonomy: ProjectAutonomy
  setBy: string | null
  setAt: string | null
}

export function getProjectAutonomyKey(projectId: string) {
  return `v1/projects/${projectId}/autonomy`
}

type GetProjectAutonomyResponse = {
  autonomy: ProjectAutonomySetting
}

export function getProjectAutonomy(projectId: string) {
  return apiClient
    .get(getProjectAutonomyKey(projectId))
    .json<GetProjectAutonomyResponse>()
}

type SetProjectAutonomyResponse = {
  project: Project
  autonomy: ProjectAutonomySetting
}

// Members alone: a guest is refused, and no agent can turn it. It applies to what the Project
// Manager submits and changes from then on.
export function setProjectAutonomy(projectId: string, autonomy: ProjectAutonomy) {
  return apiClient
    .put(getProjectAutonomyKey(projectId), { json: { autonomy }})
    .json<SetProjectAutonomyResponse>()
}

type ProjectCoverResponse = {
  project: Project
}

// Sends the file itself as the body. The API compresses it and answers 400 for anything
// that is not a PNG, JPEG, WebP, or GIF image under 10 MB.
export function uploadProjectCover(projectId: string, file: File) {
  return apiClient
    .put(`v1/projects/${projectId}/cover`, { body: file, timeout: PROJECT_COVER_UPLOAD_TIMEOUT_MS })
    .json<ProjectCoverResponse>()
}

// Removing is soft: the API keeps the image until a new cover replaces it.
export function deleteProjectCover(projectId: string) {
  return apiClient
    .delete(`v1/projects/${projectId}/cover`)
    .json<ProjectCoverResponse>()
}

// Shows a removed cover again. Without one, it answers the project as it is.
export function restoreProjectCover(projectId: string) {
  return apiClient
    .post(`v1/projects/${projectId}/cover/restore`)
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
