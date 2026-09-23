-- Provenance graph storage is deliberately namespaced and migrated separately
-- from items/items_fts and the sqlite-vec tables.
CREATE TABLE IF NOT EXISTS graph_schema_version (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    version INTEGER NOT NULL
);
INSERT OR IGNORE INTO graph_schema_version(singleton, version) VALUES (1, 1);

CREATE TABLE IF NOT EXISTS graph_builds (
    id TEXT PRIMARY KEY,
    started_at TEXT NOT NULL,
    completed_at TEXT,
    state TEXT NOT NULL CHECK (state IN ('building', 'completed', 'failed')),
    extractor_model TEXT,
    extractor_version TEXT,
    git_head TEXT
);
CREATE INDEX IF NOT EXISTS graph_builds_state_completed
    ON graph_builds(state, completed_at);

CREATE TABLE IF NOT EXISTS graph_sources (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    human_maintained INTEGER NOT NULL DEFAULT 0 CHECK (human_maintained IN (0, 1)),
    active INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    latest_content_hash TEXT
);
CREATE INDEX IF NOT EXISTS graph_sources_active_path
    ON graph_sources(active, path);

CREATE TABLE IF NOT EXISTS graph_source_versions (
    id INTEGER PRIMARY KEY,
    source_id INTEGER NOT NULL REFERENCES graph_sources(id),
    content_hash TEXT NOT NULL,
    git_head TEXT,
    snapshot TEXT NOT NULL,
    extraction_version TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(source_id, content_hash),
    UNIQUE(id, source_id)
);
CREATE INDEX IF NOT EXISTS graph_source_versions_source
    ON graph_source_versions(source_id, created_at);

-- Explicit membership makes a build's selected source set immutable. Readers
-- can therefore ignore incomplete builds and use only the latest completed one.
CREATE TABLE IF NOT EXISTS graph_build_sources (
    build_id TEXT NOT NULL REFERENCES graph_builds(id) ON DELETE CASCADE,
    source_id INTEGER NOT NULL,
    source_version_id INTEGER NOT NULL,
    PRIMARY KEY(build_id, source_id),
    FOREIGN KEY(source_version_id, source_id)
        REFERENCES graph_source_versions(id, source_id)
);
CREATE INDEX IF NOT EXISTS graph_build_sources_version
    ON graph_build_sources(source_version_id);

CREATE TABLE IF NOT EXISTS graph_spans (
    id TEXT PRIMARY KEY,
    source_version_id INTEGER NOT NULL REFERENCES graph_source_versions(id),
    heading_path TEXT NOT NULL DEFAULT '',
    start_line INTEGER NOT NULL CHECK (start_line >= 1),
    end_line INTEGER NOT NULL CHECK (end_line >= start_line),
    text TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS graph_spans_version_lines
    ON graph_spans(source_version_id, start_line, end_line);

CREATE TABLE IF NOT EXISTS graph_nodes (
    id INTEGER PRIMARY KEY,
    canonical_key TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL,
    display_name TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS graph_nodes_canonical_key
    ON graph_nodes(canonical_key);

CREATE TABLE IF NOT EXISTS graph_aliases (
    id INTEGER PRIMARY KEY,
    node_id INTEGER NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    alias TEXT NOT NULL,
    normalized_alias TEXT NOT NULL,
    UNIQUE(node_id, kind, normalized_alias)
);
CREATE INDEX IF NOT EXISTS graph_aliases_kind_normalized
    ON graph_aliases(kind, normalized_alias);
CREATE INDEX IF NOT EXISTS graph_aliases_node
    ON graph_aliases(node_id);

CREATE TABLE IF NOT EXISTS graph_claims (
    id TEXT PRIMARY KEY,
    subject_node_id INTEGER NOT NULL REFERENCES graph_nodes(id),
    predicate TEXT NOT NULL,
    object_node_id INTEGER REFERENCES graph_nodes(id),
    object_literal TEXT,
    polarity TEXT NOT NULL CHECK (polarity IN ('positive', 'negative')),
    modality TEXT NOT NULL,
    status TEXT NOT NULL,
    scope TEXT NOT NULL,
    source_version_id INTEGER NOT NULL REFERENCES graph_source_versions(id),
    build_id TEXT NOT NULL REFERENCES graph_builds(id),
    claim_text TEXT NOT NULL,
    active INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    CHECK ((object_node_id IS NOT NULL) <> (object_literal IS NOT NULL))
);
CREATE INDEX IF NOT EXISTS graph_claims_subject_predicate
    ON graph_claims(subject_node_id, predicate);
CREATE INDEX IF NOT EXISTS graph_claims_predicate_object
    ON graph_claims(predicate, object_node_id);
CREATE INDEX IF NOT EXISTS graph_claims_build_active
    ON graph_claims(build_id, active);
CREATE INDEX IF NOT EXISTS graph_claims_source_version
    ON graph_claims(source_version_id);

-- A normalized claim can belong to several completed builds. Keep its first
-- build_id as creation provenance; membership is recorded independently.
CREATE TABLE IF NOT EXISTS graph_build_claims (
    build_id TEXT NOT NULL REFERENCES graph_builds(id) ON DELETE CASCADE,
    claim_id TEXT NOT NULL REFERENCES graph_claims(id) ON DELETE CASCADE,
    PRIMARY KEY(build_id, claim_id)
);
CREATE INDEX IF NOT EXISTS graph_build_claims_claim
    ON graph_build_claims(claim_id);

CREATE TABLE IF NOT EXISTS graph_enumerations (
    id TEXT PRIMARY KEY,
    subject_node_id INTEGER NOT NULL REFERENCES graph_nodes(id),
    predicate TEXT NOT NULL,
    scope TEXT NOT NULL,
    exhaustive INTEGER NOT NULL CHECK (exhaustive IN (0, 1)),
    source_version_id INTEGER NOT NULL REFERENCES graph_source_versions(id),
    supporting_span_id TEXT NOT NULL REFERENCES graph_spans(id),
    build_id TEXT NOT NULL REFERENCES graph_builds(id)
);
CREATE INDEX IF NOT EXISTS graph_enumerations_subject_predicate
    ON graph_enumerations(subject_node_id, predicate, scope);

CREATE TABLE IF NOT EXISTS graph_build_enumerations (
    build_id TEXT NOT NULL REFERENCES graph_builds(id) ON DELETE CASCADE,
    enumeration_id TEXT NOT NULL REFERENCES graph_enumerations(id) ON DELETE CASCADE,
    exhaustive INTEGER NOT NULL CHECK (exhaustive IN (0, 1)),
    PRIMARY KEY(build_id, enumeration_id)
);
CREATE INDEX IF NOT EXISTS graph_build_enumerations_enumeration
    ON graph_build_enumerations(enumeration_id);

CREATE TABLE IF NOT EXISTS graph_enumeration_members (
    enumeration_id TEXT NOT NULL REFERENCES graph_enumerations(id) ON DELETE CASCADE,
    claim_id TEXT NOT NULL REFERENCES graph_claims(id) ON DELETE CASCADE,
    PRIMARY KEY(enumeration_id, claim_id)
);
CREATE INDEX IF NOT EXISTS graph_enumeration_members_claim
    ON graph_enumeration_members(claim_id);

CREATE TABLE IF NOT EXISTS graph_build_enumeration_members (
    build_id TEXT NOT NULL REFERENCES graph_builds(id) ON DELETE CASCADE,
    enumeration_id TEXT NOT NULL REFERENCES graph_enumerations(id) ON DELETE CASCADE,
    claim_id TEXT NOT NULL REFERENCES graph_claims(id) ON DELETE CASCADE,
    PRIMARY KEY(build_id, enumeration_id, claim_id),
    FOREIGN KEY(build_id, enumeration_id)
        REFERENCES graph_build_enumerations(build_id, enumeration_id),
    FOREIGN KEY(build_id, claim_id)
        REFERENCES graph_build_claims(build_id, claim_id)
);
CREATE INDEX IF NOT EXISTS graph_build_enumeration_members_claim
    ON graph_build_enumeration_members(build_id, claim_id);

CREATE TABLE IF NOT EXISTS graph_claim_evidence (
    claim_id TEXT NOT NULL REFERENCES graph_claims(id) ON DELETE CASCADE,
    span_id TEXT NOT NULL REFERENCES graph_spans(id),
    PRIMARY KEY(claim_id, span_id)
);
CREATE INDEX IF NOT EXISTS graph_claim_evidence_span
    ON graph_claim_evidence(span_id, claim_id);

CREATE TABLE IF NOT EXISTS graph_claim_supersession (
    new_claim_id TEXT NOT NULL REFERENCES graph_claims(id),
    old_claim_id TEXT NOT NULL REFERENCES graph_claims(id),
    reason TEXT NOT NULL,
    PRIMARY KEY(new_claim_id, old_claim_id),
    CHECK (new_claim_id <> old_claim_id)
);
CREATE INDEX IF NOT EXISTS graph_claim_supersession_old
    ON graph_claim_supersession(old_claim_id);

-- This FTS table is derived from claim records and aliases. Alias rows are
-- emitted against claims involving the aliased node, so candidate retrieval
-- can return claim IDs for either claim wording or entity aliases.
CREATE VIRTUAL TABLE IF NOT EXISTS graph_claims_fts USING fts5(
    claim_id UNINDEXED,
    alias_id UNINDEXED,
    claim_text,
    alias_text
);

CREATE TRIGGER IF NOT EXISTS graph_claims_fts_ai AFTER INSERT ON graph_claims BEGIN
    INSERT INTO graph_claims_fts(claim_id, alias_id, claim_text, alias_text)
        VALUES (new.id, NULL, new.claim_text, '');
    INSERT INTO graph_claims_fts(claim_id, alias_id, claim_text, alias_text)
        SELECT new.id, id, '', alias FROM graph_aliases
        WHERE node_id = new.subject_node_id OR node_id = new.object_node_id;
END;
CREATE TRIGGER IF NOT EXISTS graph_claims_fts_ad AFTER DELETE ON graph_claims BEGIN
    DELETE FROM graph_claims_fts WHERE claim_id = old.id;
END;
CREATE TRIGGER IF NOT EXISTS graph_claims_fts_au AFTER UPDATE OF claim_text, subject_node_id, object_node_id
ON graph_claims BEGIN
    DELETE FROM graph_claims_fts WHERE claim_id = old.id;
    INSERT INTO graph_claims_fts(claim_id, alias_id, claim_text, alias_text)
        VALUES (new.id, NULL, new.claim_text, '');
    INSERT INTO graph_claims_fts(claim_id, alias_id, claim_text, alias_text)
        SELECT new.id, id, '', alias FROM graph_aliases
        WHERE node_id = new.subject_node_id OR node_id = new.object_node_id;
END;

CREATE TRIGGER IF NOT EXISTS graph_aliases_fts_ai AFTER INSERT ON graph_aliases BEGIN
    INSERT INTO graph_claims_fts(claim_id, alias_id, claim_text, alias_text)
        SELECT id, new.id, '', new.alias FROM graph_claims
        WHERE subject_node_id = new.node_id OR object_node_id = new.node_id;
END;
CREATE TRIGGER IF NOT EXISTS graph_aliases_fts_ad AFTER DELETE ON graph_aliases BEGIN
    DELETE FROM graph_claims_fts
        WHERE alias_id = old.id;
END;
CREATE TRIGGER IF NOT EXISTS graph_aliases_fts_au AFTER UPDATE OF alias, node_id ON graph_aliases BEGIN
    DELETE FROM graph_claims_fts
        WHERE alias_id = old.id;
    INSERT INTO graph_claims_fts(claim_id, alias_id, claim_text, alias_text)
        SELECT id, new.id, '', new.alias FROM graph_claims
        WHERE subject_node_id = new.node_id OR object_node_id = new.node_id;
END;
