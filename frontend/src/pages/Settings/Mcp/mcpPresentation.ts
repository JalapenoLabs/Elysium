// Copyright © 2026 Jalapeno Labs

// The commands that add Elysium's MCP server to each client, as its own docs spell them.
// `name` is what the client calls the server; `url` is the server's address.
export const connectCommandByClient = {
  claudeCode: (name: string, url: string) => `claude mcp add --transport http ${name} ${url}`,
  codex: (name: string, url: string) => `codex mcp add ${name} --url ${url}`,
} as const satisfies Record<string, (name: string, url: string) => string>

export type McpClientKind = keyof typeof connectCommandByClient

// The MCP server's address on the origin this page was loaded from.
export function mcpServerUrl(origin: string) {
  return `${origin}/api/mcp`
}

// The name a client registers the server under: the brand, lowercased, with anything a shell or
// a client's config would trip on taken out.
export function mcpServerName(brand: string) {
  return brand
    .toLowerCase()
    .replace(/[^a-z0-9-]+/g, '-')
    .replace(/^-+|-+$/g, '')
}
