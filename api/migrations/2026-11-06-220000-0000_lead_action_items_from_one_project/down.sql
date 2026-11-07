ALTER TABLE action_items
    DROP CONSTRAINT action_items_lead_project_is_a_membership,
    DROP COLUMN lead_project_id;

ALTER TABLE action_item_projects
    DROP CONSTRAINT action_item_projects_attributed_by_length,
    DROP CONSTRAINT action_item_projects_confidence_range,
    DROP COLUMN attributed_by,
    DROP COLUMN confidence;
