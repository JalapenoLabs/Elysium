DELETE FROM knowledge_backfills WHERE source = 'turn_summary';
ALTER TABLE knowledge_backfills DROP CONSTRAINT knowledge_backfills_source_check;
ALTER TABLE knowledge_backfills ADD CONSTRAINT knowledge_backfills_source_check
    CHECK (source IN ('timeline_record'));

DROP INDEX turn_summaries_turn_id;

-- Summaries' records exist only through this migration; their documents and attributions go with
-- them.
DELETE FROM timeline_records WHERE turn_summary_turn_id IS NOT NULL;

ALTER TABLE timeline_records
    DROP CONSTRAINT timeline_records_turn_summary_kind,
    DROP CONSTRAINT timeline_records_one_per_turn_summary,
    DROP CONSTRAINT timeline_records_turn_summary_fkey,
    DROP COLUMN turn_summary_turn_id,
    DROP COLUMN turn_summary_session_id;
