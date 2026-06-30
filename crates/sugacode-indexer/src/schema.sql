CREATE TABLE IF NOT EXISTS repo_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS items (
    id INTEGER PRIMARY KEY,
    source_type TEXT NOT NULL,
    identifier TEXT NOT NULL,
    text TEXT NOT NULL,
    author TEXT,
    metadata TEXT,
    UNIQUE(source_type, identifier)
);

CREATE VIRTUAL TABLE IF NOT EXISTS items_fts USING fts5(
    text, author,
    content='items', content_rowid='id'
);

CREATE TRIGGER IF NOT EXISTS items_ai AFTER INSERT ON items BEGIN
    INSERT INTO items_fts(rowid, text, author) VALUES (new.id, new.text, new.author);
END;

CREATE TRIGGER IF NOT EXISTS items_ad AFTER DELETE ON items BEGIN
    INSERT INTO items_fts(items_fts, rowid, text, author) VALUES('delete', old.id, old.text, old.author);
END;

CREATE TRIGGER IF NOT EXISTS items_au AFTER UPDATE ON items BEGIN
    INSERT INTO items_fts(items_fts, rowid, text, author) VALUES('delete', old.id, old.text, old.author);
    INSERT INTO items_fts(rowid, text, author) VALUES (new.id, new.text, new.author);
END;
