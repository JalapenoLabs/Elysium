# Confluence connector

Read this before changing anything in `api/src/confluence/`, and add to it whenever Confluence breaks the reader.
How the connector works is in `docs/connectors.md`, Confluence; this file is what the next person to fix it needs.

## How it reads Confluence, and why

- Three calls, all on the account's own site with basic auth (email and a classic API token):
  `GET /wiki/rest/api/user/current`, `GET /wiki/api/v2/spaces`, and `GET /wiki/rest/api/content/search`.
- Content is read through v1's search, not v2, because only v1 takes CQL with `lastmodified` and the `space in (...)`
  bound, and expands bodies, versions, and containers in one call.
- `mod.rs` deserializes only the fields it uses, each typed as the OpenAPI specification declares it. Unknown fields
  are ignored, so a field Confluence adds never breaks a read; a field whose type changes does, and fails the whole
  search, which is why types matter more than anything else here.

## Atlassian's specifications

- v1: `https://dac-static.atlassian.com/cloud/confluence/swagger.v3.json`
- v2: `https://dac-static.atlassian.com/cloud/confluence/openapi-v2.v3.json`

Check a field's type there before adding it. Shapes a reader meets:

| Field                                   | Type                                                              |
|-----------------------------------------|-------------------------------------------------------------------|
| v1 `Content.id`                         | string                                                            |
| v1 `Space.id`                           | integer (int64), such as `131075`                                 |
| v1 `Content.container`                  | an open object: the page for a comment, the space for a page     |
| v1 `Version.number`                     | integer; `when` is a nullable date-time string                    |
| v1 `User.accountId`                     | nullable string                                                   |
| v1 `_links` values                      | strings (`next`, `webui`), though the spec allows objects         |
| v2 `SpaceBulk.id`, `homepageId`         | string                                                            |

## Quirks

- v1 and v2 disagree on ids. A v1 space's id is a number and a v2 space's is a string, so an id that may be a space
  goes through `string_or_number`.
- A result's `container` follows its type. A page's container is its space, so treating it as a page names the space
  as the page (`pageId`) and the title as `Re: <space name>`. Only a comment's container is kept.
- CQL reads a written date in the account's own time zone, so windows are relative (`now("-Nm")`).
- A scoped API token is refused on the site's own host and works only through
  `api.atlassian.com/ex/confluence/<cloudId>`, which the connector does not call yet.

## Breakages

### A page's container is its space, whose id is a number (issue #68, November 2026)

- Symptom: no read recorded anything, and the connector showed:

  > Confluence sent search results that cannot be read: invalid type: integer `131075`, expected a string at line 1 column 2805

- Cause: `ContainerBody.id` was a `String`. The connector was written from the documented shapes before it met a live
  site, assuming every container is a page. A search holding any page failed whole, because its container is the
  space, and v1 sends a space's id as a number.
- Fix: the container's id reads a string or a number, and `into_change` keeps the container for comments alone.
  `bug_68_a_page_whose_container_is_its_space_is_read` in `tests.rs` holds a page as Confluence Cloud sends it, and
  the fake's `page()` now carries its space as its container, so every end to end test meets the real shape.
