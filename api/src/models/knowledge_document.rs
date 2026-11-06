// Copyright © 2026 Jalapeno Labs

//! The knowledge index's documents and passages: how every source enters and leaves it.
//!
//! A source is upserted as one document whose passages are its text in order ([`upsert`]), and
//! removed with [`remove`]. Both take the connection of the caller's transaction, so a source and
//! its document change together: what a save commits is searchable at once, and a source that
//! fails to save leaves nothing indexed.
//!
//! Each kind of source is a column of `knowledge_documents` (`docs/database.md`), and
//! [`KnowledgeSource`] is the one place that maps a kind to its column: a timeline record's or a
//! page's, learnings and the company doc's sections being pages. Indexing a source again
//! is cheap when nothing changed: the SHA-256 of its text says so and its passages are left
//! alone. When something did, passages are rewritten by position, and one whose text held keeps
//! its embedding while one whose text moved waits for the embedding model again.
//!
//! [`get`] reads one document whole, as a citation opens it; the search that finds documents is
//! `crate::knowledge::search`.

use chrono::{DateTime, Utc};
use diesel::dsl::sql;
use diesel::prelude::*;
use diesel::sql_types::{Nullable, Text};
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::database::schema::sql_types::Vector;
use crate::database::schema::{
    knowledge_documents, knowledge_passages, projects, timeline_record_projects, users,
};
use crate::embeddings::Embedding;
use crate::models::company_doc::Section;
use crate::models::{page, page_label};

/// What a document was made from. Each kind is a column of its own, unique and cascading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KnowledgeSource {
    /// The timeline record with this id, of this kind (`timeline_records.source_kind`), which
    /// the search's source filter names it by, such as `slack` or `meeting`.
    TimelineRecord { id: Uuid, kind: String },
    /// The page with this id, of this kind, which the search's source filter names it by, as a
    /// timeline record's kind is: `page`, and a kind of its own for a page that stands for
    /// something a filter already names, `learning` and `company_doc`.
    Page { id: Uuid, kind: String },
}

impl KnowledgeSource {
    /// What the search's source filter calls documents of this kind.
    pub fn label(&self) -> &str {
        match self {
            Self::TimelineRecord { kind, .. } | Self::Page { kind, .. } => kind,
        }
    }
}

/// A source as the index holds it.
#[derive(Debug, Clone)]
pub struct IndexedDocument {
    pub source: KnowledgeSource,
    /// How a citation names it.
    pub title: String,
    /// Where a citation links to, when the source lives outside Elysium.
    pub url: Option<String>,
    /// When what it says was said, for the date filter.
    pub occurred_at: DateTime<Utc>,
    /// Whose words they are, for the person filter and the citation. Never access control.
    pub created_by: Uuid,
    /// Its text in order, as [`crate::knowledge::passages::split`] cuts it. Each is 1 to 20,000
    /// characters.
    pub passages: Vec<String>,
}

#[derive(Insertable, AsChangeset)]
#[diesel(table_name = knowledge_documents)]
struct DocumentFields {
    source: String,
    title: String,
    // A document whose source has no link of its own clears the one it had.
    #[diesel(treat_none_as_null = true)]
    url: Option<String>,
    occurred_at: DateTime<Utc>,
    content_sha256: String,
    created_by: Uuid,
}

#[derive(Insertable)]
#[diesel(table_name = knowledge_passages)]
struct NewPassage<'a> {
    document_id: Uuid,
    position: i32,
    content: &'a str,
}

/// The hex SHA-256 the document's text is recorded under. Each passage is followed by a byte no
/// text contains, so moving words across a passage boundary changes it too.
pub(crate) fn content_sha256(passages: &[String]) -> String {
    let mut hasher = Sha256::new();
    for passage in passages {
        hasher.update(passage.as_bytes());
        hasher.update([0x1e]);
    }
    hex::encode(hasher.finalize())
}

/// The document made from `source`, locked until the transaction ends, with the SHA-256 of the
/// text it was indexed from.
async fn lock(
    connection: &mut AsyncPgConnection,
    source: &KnowledgeSource,
) -> QueryResult<Option<(Uuid, String)>> {
    let query = knowledge_documents::table
        .select((knowledge_documents::id, knowledge_documents::content_sha256))
        .for_update();
    match source {
        KnowledgeSource::TimelineRecord { id, .. } => query
            .filter(knowledge_documents::timeline_record_id.eq(id))
            .first(connection)
            .await
            .optional(),
        KnowledgeSource::Page { id, .. } => query
            .filter(knowledge_documents::page_id.eq(id))
            .first(connection)
            .await
            .optional(),
    }
}

/// Inserts a new document for `source`, naming it in its column.
async fn insert(
    connection: &mut AsyncPgConnection,
    source: &KnowledgeSource,
    fields: DocumentFields,
) -> QueryResult<Uuid> {
    let id = Uuid::now_v7();
    let insert = diesel::insert_into(knowledge_documents::table);
    match source {
        KnowledgeSource::TimelineRecord {
            id: timeline_record_id,
            ..
        } => {
            insert
                .values((
                    knowledge_documents::id.eq(id),
                    knowledge_documents::timeline_record_id.eq(timeline_record_id),
                    fields,
                ))
                .execute(connection)
                .await?;
        }
        KnowledgeSource::Page { id: page_id, .. } => {
            insert
                .values((
                    knowledge_documents::id.eq(id),
                    knowledge_documents::page_id.eq(page_id),
                    fields,
                ))
                .execute(connection)
                .await?;
        }
    }
    Ok(id)
}

/// Indexes `document`: inserts its source's document, or updates the one there, and rewrites
/// its passages if its text changed. Answers the document's id.
///
/// Call it inside the transaction that changes the source. Two transactions indexing one source
/// at once must hold the source's own row lock, as every caller's write does; the document's
/// row is locked here as well, for anything that reaches it some other way.
///
/// # Errors
/// Propagates any database error, including a foreign key violation for a source that does not
/// exist and a check violation for a passage that is empty or longer than 20,000 characters.
pub async fn upsert(
    connection: &mut AsyncPgConnection,
    document: &IndexedDocument,
) -> QueryResult<Uuid> {
    let sha256 = content_sha256(&document.passages);
    let fields = DocumentFields {
        source: document.source.label().to_owned(),
        title: document.title.clone(),
        url: document.url.clone(),
        occurred_at: document.occurred_at,
        content_sha256: sha256.clone(),
        created_by: document.created_by,
    };

    let Some((id, indexed_sha256)) = lock(connection, &document.source).await? else {
        let id = insert(connection, &document.source, fields).await?;
        write_passages(connection, id, &document.passages).await?;
        return Ok(id);
    };

    diesel::update(knowledge_documents::table.find(id))
        .set(fields)
        .execute(connection)
        .await?;
    if indexed_sha256 != sha256 {
        write_passages(connection, id, &document.passages).await?;
    }
    Ok(id)
}

/// Removes `source`'s document and passages from the index. Removing one that is not indexed
/// changes nothing.
///
/// A source deleted from its table needs no call: its document cascades with it. This is for a
/// source that stays but leaves the index, such as a learning or a page deleted softly, or the
/// record of a Confluence page's older version once a newer one is recorded.
///
/// # Errors
/// Propagates any database error.
pub async fn remove(
    connection: &mut AsyncPgConnection,
    source: &KnowledgeSource,
) -> QueryResult<()> {
    let documents = knowledge_documents::table;
    match source {
        KnowledgeSource::TimelineRecord { id, .. } => {
            diesel::delete(documents.filter(knowledge_documents::timeline_record_id.eq(id)))
                .execute(connection)
                .await?;
        }
        KnowledgeSource::Page { id, .. } => {
            diesel::delete(documents.filter(knowledge_documents::page_id.eq(id)))
                .execute(connection)
                .await?;
        }
    }
    Ok(())
}

/// Whose words a document holds: the person whose record, learning, edit, or version it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub id: Uuid,
    pub name: String,
}

/// How a document is named and linked to wherever it is cited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Citation {
    pub document_id: Uuid,
    /// The search's source filter's name for it, such as `slack`, `learning`, or `company_doc`.
    pub source: String,
    pub title: String,
    /// Where it lives outside Elysium, when it does.
    pub url: Option<String>,
    /// The page it was indexed from, a learning's or a company doc section's included, which a
    /// reader opens in place of the document.
    pub page_id: Option<Uuid>,
    pub occurred_at: DateTime<Utc>,
    pub person: Person,
}

/// Another document of the same source holding the same text: two people's connectors read the
/// same channel or space, and each has the record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Copy {
    pub document_id: Uuid,
    pub person: Person,
}

/// One passage of a document, at its place in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Queryable)]
#[serde(rename_all = "camelCase")]
pub struct Passage {
    pub position: i32,
    pub content: String,
}

/// What a document was made from, as a reader opens it in Elysium.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Origin {
    /// A timeline record. The record itself stays its owner's (`docs/knowledge.md`, The index is
    /// shared; records stay their owner's), so the document is what a reader opens, and the
    /// record's id is not given.
    TimelineRecord,
    Learning {
        learning_id: Uuid,
    },
    /// A company doc section, by its key and the page that holds it.
    CompanyDoc {
        section: String,
        page_id: Uuid,
    },
    /// A knowledge base page, which a reader opens by its id.
    Page {
        page_id: Uuid,
    },
}

/// A project a timeline record is attributed to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Queryable)]
#[serde(rename_all = "camelCase")]
pub struct ProjectReference {
    pub id: Uuid,
    pub name: String,
}

/// One document whole, as a citation opens it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentView {
    pub citation: Citation,
    pub origin: Origin,
    /// Every passage, in order, or those of the span a reader asked for.
    pub passages: Vec<Passage>,
    /// How many passages the document has, whichever of them were answered.
    pub passage_count: usize,
    /// The projects its timeline record is attributed to, by name; none for any other source.
    pub projects: Vec<ProjectReference>,
    /// The labels its page carries, in order; none for any other source.
    pub labels: Vec<String>,
    /// Others whose records hold the same text, earliest first.
    pub also_recorded_by: Vec<Copy>,
}

/// A document's row as [`get`] reads it, with its person's name.
#[derive(Queryable)]
struct DocumentRow {
    id: Uuid,
    source: String,
    title: String,
    url: Option<String>,
    occurred_at: DateTime<Utc>,
    created_by: Uuid,
    person_name: String,
    content_sha256: String,
    timeline_record_id: Option<Uuid>,
    page_id: Option<Uuid>,
}

/// The passage at `position` of page `page_id`'s document, its text as the search cited it. `None`
/// when the index holds no such passage: the page says nothing, or is deleted, or a link names a
/// place its text no longer reaches.
///
/// # Errors
/// Propagates any database error.
pub async fn passage_of_page(
    connection: &mut AsyncPgConnection,
    page_id: Uuid,
    position: i32,
) -> QueryResult<Option<Passage>> {
    knowledge_passages::table
        .inner_join(knowledge_documents::table)
        .filter(knowledge_documents::page_id.eq(page_id))
        .filter(knowledge_passages::position.eq(position))
        .select((knowledge_passages::position, knowledge_passages::content))
        .first(connection)
        .await
        .optional()
}

/// The document `id` whole: its citation, what it was made from, every passage in order, its
/// projects, and the other documents holding the same text. `None` when the index holds no such
/// document, such as one whose source was deleted.
///
/// It answers any document: callers check that whoever asks is one of the company's members
/// (`docs/knowledge.md`, Who sees what).
///
/// # Errors
/// Propagates any database error.
pub async fn get(
    connection: &mut AsyncPgConnection,
    id: Uuid,
) -> QueryResult<Option<DocumentView>> {
    let row: Option<DocumentRow> = knowledge_documents::table
        .inner_join(users::table)
        .filter(knowledge_documents::id.eq(id))
        .select((
            knowledge_documents::id,
            knowledge_documents::source,
            knowledge_documents::title,
            knowledge_documents::url,
            knowledge_documents::occurred_at,
            knowledge_documents::created_by,
            users::name,
            knowledge_documents::content_sha256,
            knowledge_documents::timeline_record_id,
            knowledge_documents::page_id,
        ))
        .first(connection)
        .await
        .optional()?;
    let Some(row) = row else {
        return Ok(None);
    };

    let passages: Vec<Passage> = knowledge_passages::table
        .filter(knowledge_passages::document_id.eq(row.id))
        .select((knowledge_passages::position, knowledge_passages::content))
        .order(knowledge_passages::position.asc())
        .load(connection)
        .await?;

    let projects: Vec<ProjectReference> = match row.timeline_record_id {
        Some(record_id) => {
            timeline_record_projects::table
                .inner_join(projects::table)
                .filter(timeline_record_projects::timeline_record_id.eq(record_id))
                .select((projects::id, projects::name))
                .order(projects::name.asc())
                .load(connection)
                .await?
        }
        None => Vec::new(),
    };

    let labels = match row.page_id {
        Some(page_id) => page_label::of_page(connection, page_id).await?,
        None => Vec::new(),
    };

    // Through `knowledge_documents_copies`, so copies are found however large the index grows.
    // Two pages saying the same thing are two pages, so only records have copies.
    let copies: Vec<(Uuid, Uuid, String)> = knowledge_documents::table
        .inner_join(users::table)
        .filter(knowledge_documents::content_sha256.eq(&row.content_sha256))
        .filter(knowledge_documents::source.eq(&row.source))
        .filter(knowledge_documents::id.ne(row.id))
        .filter(knowledge_documents::page_id.is_null())
        .select((
            knowledge_documents::id,
            knowledge_documents::created_by,
            users::name,
        ))
        .order((
            knowledge_documents::occurred_at.asc(),
            knowledge_documents::id.asc(),
        ))
        .load(connection)
        .await?;

    let origin = row.page_id.map_or(Origin::TimelineRecord, |page_id| {
        origin_of_page(page_id, &row.source)
    });

    Ok(Some(DocumentView {
        citation: Citation {
            document_id: row.id,
            source: row.source,
            title: row.title,
            url: row.url,
            page_id: row.page_id,
            occurred_at: row.occurred_at,
            person: Person {
                id: row.created_by,
                name: row.person_name,
            },
        },
        origin,
        passage_count: passages.len(),
        passages,
        projects,
        labels,
        also_recorded_by: copies
            .into_iter()
            .map(|(document_id, person_id, name)| Copy {
                document_id,
                person: Person {
                    id: person_id,
                    name,
                },
            })
            .collect(),
    }))
}

/// What a page's document opens as: a learning and a section stay named as such, which is how their
/// citations open them, and the rest as the page they are. A learning's id is its page's.
fn origin_of_page(page_id: Uuid, source: &str) -> Origin {
    if let Some(section) = Section::of_page(page_id) {
        return Origin::CompanyDoc {
            section: section.as_str().to_owned(),
            page_id,
        };
    }
    if source == page::LEARNING_INDEX_KIND {
        return Origin::Learning {
            learning_id: page_id,
        };
    }
    Origin::Page { page_id }
}

/// A passage the embedding model has still to embed, as it read when it was taken.
#[derive(Debug, Clone, PartialEq, Eq, Queryable)]
pub struct UnembeddedPassage {
    pub document_id: Uuid,
    pub position: i32,
    pub content: String,
}

/// Up to `limit` passages waiting for the embedding model, newest documents first, through the
/// partial index `knowledge_passages_unembedded`.
///
/// Newest first, so what was just saved is found by meaning within a pass even while a
/// backfill of older sources waits.
///
/// # Errors
/// Propagates any database error.
pub async fn unembedded(
    connection: &mut AsyncPgConnection,
    limit: i64,
) -> QueryResult<Vec<UnembeddedPassage>> {
    knowledge_passages::table
        .filter(knowledge_passages::embedding.is_null())
        .select((
            knowledge_passages::document_id,
            knowledge_passages::position,
            knowledge_passages::content,
        ))
        // Both descending, so the index is read backwards rather than sorted.
        .order((
            knowledge_passages::document_id.desc(),
            knowledge_passages::position.desc(),
        ))
        .limit(limit)
        .load(connection)
        .await
}

/// Records each passage's embedding as made by `model`, in one transaction, and answers how
/// many were recorded.
///
/// A passage is written only while its text is still what was embedded: indexing its source
/// again may have rewritten it since it was taken ([`upsert`]), and that text waits for an
/// embedding of its own. A passage whose document is gone is likewise left out.
///
/// # Errors
/// Propagates any database error.
pub async fn store_embeddings(
    connection: &mut AsyncPgConnection,
    model: &str,
    embedded: &[(UnembeddedPassage, Embedding)],
) -> QueryResult<usize> {
    connection
        .transaction(async |connection| {
            let mut stored = 0;
            for (passage, embedding) in embedded {
                // No Rust type stands for pgvector's `vector`, so the embedding travels as
                // pgvector's text form and is cast where it lands.
                let vector = sql::<Nullable<Vector>>("")
                    .bind::<Text, _>(embedding.to_pgvector())
                    .sql("::vector");
                stored += diesel::update(
                    knowledge_passages::table.find((passage.document_id, passage.position)),
                )
                .filter(knowledge_passages::content.eq(&passage.content))
                .set((
                    knowledge_passages::embedding.eq(vector),
                    knowledge_passages::embedding_model.eq(model),
                ))
                .execute(connection)
                .await?;
            }
            Ok(stored)
        })
        .await
}

/// Forgets every embedding made by a model other than `model`, so each of those passages
/// waits for `model` again, and answers how many there were.
///
/// Vectors of two models are not comparable: a query embedded by one ranks the other's at
/// random. So a passage whose embedding the current model did not make is treated as not
/// embedded at all, and found by its words alone until it is embedded again.
///
/// # Errors
/// Propagates any database error.
pub async fn forget_embeddings_not_made_by(
    connection: &mut AsyncPgConnection,
    model: &str,
) -> QueryResult<usize> {
    // Before or after `model` rather than not `model`: a btree answers the two ranges from
    // `knowledge_passages_embedding_model`, but never an inequality, so this stays an index scan
    // however many passages the current model has embedded.
    let made_by_another = knowledge_passages::embedding_model
        .lt(model)
        .or(knowledge_passages::embedding_model.gt(model));
    diesel::update(knowledge_passages::table.filter(made_by_another))
        .set((
            knowledge_passages::embedding.eq(sql::<Nullable<Vector>>("NULL")),
            knowledge_passages::embedding_model.eq(None::<String>),
        ))
        .execute(connection)
        .await
}

/// Rewrites a document's passages by position: one whose text held is left as it is, embedding
/// and all; one whose text moved takes the new text and loses its embedding until the model
/// embeds it again; positions past the new end are deleted.
async fn write_passages(
    connection: &mut AsyncPgConnection,
    document_id: Uuid,
    passages: &[String],
) -> QueryResult<()> {
    let indexed: Vec<(i32, String)> = knowledge_passages::table
        .filter(knowledge_passages::document_id.eq(document_id))
        .select((knowledge_passages::position, knowledge_passages::content))
        .order(knowledge_passages::position.asc())
        .load(connection)
        .await?;

    let mut inserts = Vec::new();
    for (index, content) in passages.iter().enumerate() {
        let position = i32::try_from(index).expect("a document has fewer than 2^31 passages");
        match indexed.get(index) {
            Some((_, indexed_content)) if indexed_content == content => {}
            Some(_) => {
                diesel::update(knowledge_passages::table.find((document_id, position)))
                    .set((
                        knowledge_passages::content.eq(content),
                        // No Rust type stands for pgvector's `vector`, so the NULL is written
                        // as SQL of the column's type.
                        knowledge_passages::embedding.eq(sql::<Nullable<Vector>>("NULL")),
                        knowledge_passages::embedding_model.eq(None::<String>),
                    ))
                    .execute(connection)
                    .await?;
            }
            None => inserts.push(NewPassage {
                document_id,
                position,
                content,
            }),
        }
    }
    if !inserts.is_empty() {
        diesel::insert_into(knowledge_passages::table)
            .values(inserts)
            .execute(connection)
            .await?;
    }

    let kept = i32::try_from(passages.len()).expect("a document has fewer than 2^31 passages");
    diesel::delete(
        knowledge_passages::table
            .filter(knowledge_passages::document_id.eq(document_id))
            .filter(knowledge_passages::position.ge(kept)),
    )
    .execute(connection)
    .await?;
    Ok(())
}
