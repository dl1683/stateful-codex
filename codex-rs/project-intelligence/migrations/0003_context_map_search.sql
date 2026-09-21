CREATE VIRTUAL TABLE context_map_search USING fts5(
    entry_id UNINDEXED,
    project_id UNINDEXED,
    description,
    routing_terms,
    tokenize = 'unicode61 remove_diacritics 2'
);

INSERT INTO context_map_search (
    rowid,
    entry_id,
    project_id,
    description,
    routing_terms
)
SELECT
    entry.rowid,
    entry.id,
    entry.project_id,
    entry.description,
    COALESCE(GROUP_CONCAT(term.term, ' '), '')
FROM context_map_entries AS entry
LEFT JOIN context_map_routing_terms AS term ON term.entry_id = entry.id
GROUP BY entry.rowid, entry.id, entry.project_id, entry.description;
