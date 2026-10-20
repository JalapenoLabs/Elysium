// Copyright © 2026 Jalapeno Labs

//! Room promised to Studio uploads in flight, so a location's storage limit holds under
//! concurrent uploads.
//!
//! Checking the limit and recording the file are separated by the upload itself, which can take
//! a while. Two uploads that each checked first would both find the same free room and together
//! pass the limit. So an upload into a location with a limit reserves its bytes first: under a
//! transaction-scoped advisory lock per location, it counts what the location holds plus what
//! is reserved, and records a reservation only when the file fits. The upload releases it once
//! the file is recorded or refused. A reservation a crash left behind stops counting after
//! [`RESERVATION_LIFETIME_SECONDS`]. See `docs/studio.md`, Assets.

use diesel::prelude::*;
use diesel::sql_types::{BigInt, Uuid as SqlUuid};
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::database::schema::studio_storage_reservations;
use crate::models::storage_location::StorageLocation;
use crate::models::studio_item;

/// How long a reservation counts. Long enough for a large model to stream from a satellite to a
/// provider; past it, a reservation is taken to be one a crashed upload never released.
const RESERVATION_LIFETIME_SECONDS: i64 = 60 * 60;

/// What a location has room for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Room {
    /// The location sets no limit, so nothing is reserved.
    Unbounded,
    /// The bytes are reserved; release the reservation once the upload is recorded or refused.
    Reserved(Uuid),
    /// The file does not fit; `remaining_bytes` is what is left.
    Full { remaining_bytes: u64 },
}

#[derive(Debug, Insertable)]
#[diesel(table_name = studio_storage_reservations)]
struct NewReservation {
    id: Uuid,
    storage_location_id: Uuid,
    size_bytes: i64,
    created_by: Uuid,
}

/// Reserves `size_bytes` in `location` for an upload made for `created_by`.
///
/// # Errors
/// Propagates any database error.
pub async fn reserve(
    connection: &mut AsyncPgConnection,
    location: &StorageLocation,
    size_bytes: u64,
    created_by: Uuid,
) -> QueryResult<Room> {
    let Some(limit) = location.storage_limit_bytes else {
        return Ok(Room::Unbounded);
    };
    let size = i64::try_from(size_bytes).unwrap_or(i64::MAX);
    let location_id = location.id;

    connection
        .transaction(async move |connection| {
            // Held until the transaction ends, so the count and the insert are one step for every
            // upload into this location.
            diesel::sql_query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 0))")
                .bind::<SqlUuid, _>(location_id)
                .execute(connection)
                .await?;

            let used = studio_item::bytes_in_location(connection, location_id).await?
                + reserved_bytes(connection, location_id).await?;
            let remaining = limit.saturating_sub(used);
            if size > remaining {
                return Ok(Room::Full {
                    remaining_bytes: u64::try_from(remaining).unwrap_or_default(),
                });
            }

            let reservation = NewReservation {
                id: Uuid::now_v7(),
                storage_location_id: location_id,
                size_bytes: size,
                created_by,
            };
            diesel::insert_into(studio_storage_reservations::table)
                .values(&reservation)
                .execute(connection)
                .await?;
            Ok(Room::Reserved(reservation.id))
        })
        .await
}

/// Bytes reserved in a location by uploads still in flight.
async fn reserved_bytes(
    connection: &mut AsyncPgConnection,
    storage_location_id: Uuid,
) -> QueryResult<i64> {
    #[derive(QueryableByName)]
    struct Reserved {
        #[diesel(sql_type = BigInt)]
        bytes: i64,
    }

    let reserved: Reserved = diesel::sql_query(
        "SELECT COALESCE(SUM(size_bytes), 0)::BIGINT AS bytes \
         FROM studio_storage_reservations \
         WHERE storage_location_id = $1 \
           AND created_at > now() - make_interval(secs => $2)",
    )
    .bind::<SqlUuid, _>(storage_location_id)
    .bind::<BigInt, _>(RESERVATION_LIFETIME_SECONDS)
    .get_result(connection)
    .await?;
    Ok(reserved.bytes)
}

/// Releases a reservation, once its upload is recorded or refused. Old reservations of the
/// location are cleared with it, since they no longer count.
///
/// # Errors
/// Propagates any database error.
pub async fn release(connection: &mut AsyncPgConnection, room: Room) -> QueryResult<()> {
    let Room::Reserved(id) = room else {
        return Ok(());
    };
    diesel::sql_query(
        "DELETE FROM studio_storage_reservations \
         WHERE id = $1 \
            OR created_at <= now() - make_interval(secs => $2)",
    )
    .bind::<SqlUuid, _>(id)
    .bind::<BigInt, _>(RESERVATION_LIFETIME_SECONDS)
    .execute(connection)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::storage_location;
    use crate::test_support::{
        TEST_PERSON_ID, cipher, location_for_every_project, migrated_database,
    };

    async fn location_with_limit(
        connection: &mut AsyncPgConnection,
        limit_bytes: i64,
    ) -> StorageLocation {
        let mut new_location = location_for_every_project("limited");
        new_location.storage_limit_bytes = Some(limit_bytes);
        storage_location::create(connection, &cipher(), &new_location)
            .await
            .expect("location")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn concurrent_uploads_never_reserve_past_the_limit() {
        const UPLOADS: usize = 40;
        let (url, mut connection) = migrated_database().await;
        let location = location_with_limit(&mut connection, 100).await;
        let database = crate::connections::connect_postgres(&url, 40)
            .await
            .expect("pool");

        // Forty uploads of ten bytes ask at once for a location with room for ten of them. Every
        // task is spawned before any is awaited, so they run concurrently.
        let attempts: Vec<_> = (0..UPLOADS)
            .map(|_attempt| {
                let database = database.clone();
                let location = location.clone();
                tokio::spawn(async move {
                    let mut connection = database.get().await.expect("connection");
                    reserve(&mut connection, &location, 10, TEST_PERSON_ID)
                        .await
                        .expect("reserve")
                })
            })
            .collect();
        let mut reserved = 0;
        for attempt in attempts {
            if matches!(attempt.await.expect("task"), Room::Reserved(_)) {
                reserved += 1;
            }
        }

        assert_eq!(reserved, 10, "exactly what fits is reserved");
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_released_reservation_frees_its_room() {
        let (_url, mut connection) = migrated_database().await;
        let location = location_with_limit(&mut connection, 100).await;

        let first = reserve(&mut connection, &location, 80, TEST_PERSON_ID)
            .await
            .expect("reserve");
        assert!(matches!(first, Room::Reserved(_)));
        let second = reserve(&mut connection, &location, 80, TEST_PERSON_ID)
            .await
            .expect("reserve");
        assert_eq!(
            second,
            Room::Full {
                remaining_bytes: 20
            }
        );

        release(&mut connection, first).await.expect("release");
        let third = reserve(&mut connection, &location, 80, TEST_PERSON_ID)
            .await
            .expect("reserve");
        assert!(matches!(third, Room::Reserved(_)), "the room is free again");
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_location_without_a_limit_reserves_nothing() {
        let (_url, mut connection) = migrated_database().await;
        let location = storage_location::create(
            &mut connection,
            &cipher(),
            &location_for_every_project("unbounded"),
        )
        .await
        .expect("location");
        let room = reserve(&mut connection, &location, u64::MAX, TEST_PERSON_ID)
            .await
            .expect("reserve");
        assert_eq!(room, Room::Unbounded);
    }
}
