-- Include published keywords and categories in text search.
-- Existing keyword/category columns were added in 0004.
DROP INDEX IF EXISTS packages_fts_idx;
CREATE INDEX packages_fts_idx ON packages
    USING GIN (
        to_tsvector(
            'english',
            name || ' ' || COALESCE(description, '') || ' ' ||
            keywords::text || ' ' || categories::text
        )
    );
