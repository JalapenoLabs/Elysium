-- Undoes up.sql: the Fireflies connectors it made are removed, and their keys are left without a
-- connector again. They are the ones that name no storage location, since a Fireflies connector
-- made or edited in the web app always names one; one whose person has since picked a location is
-- theirs now and stays. The meetings up.sql attached are kept without a connector again, by the
-- foreign key's ON DELETE SET NULL.
DELETE FROM connectors
WHERE kind = 'fireflies'
  AND storage_location_id IS NULL;
