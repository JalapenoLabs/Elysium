-- How a project's cover sits in its frame. `fit` shows the whole image, centered over a
-- blurred copy of itself, which suits logos and odd shapes. `fill` covers the frame and
-- crops what does not fit, which suits photos.
CREATE TYPE project_cover_fit AS ENUM ('fit', 'fill');

ALTER TABLE projects ADD COLUMN cover_fit project_cover_fit NOT NULL DEFAULT 'fit';
