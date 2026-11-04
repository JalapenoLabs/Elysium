UPDATE timeline_records
SET metadata = metadata - 'team' - 'conversation' - 'ts' - 'threadTs'
WHERE source_kind = 'slack';

UPDATE timeline_records
SET metadata = metadata - 'conversationId' - 'messageId' - 'parentId' - 'isChat'
WHERE source_kind = 'teams';
