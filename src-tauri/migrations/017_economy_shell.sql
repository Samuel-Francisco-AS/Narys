-- Presentation only; existing cognition/general settings remain untouched.
CREATE TABLE IF NOT EXISTS shell_settings (
 id INTEGER PRIMARY KEY CHECK(id=1),
 presentation_mode TEXT NOT NULL DEFAULT 'economy' CHECK(presentation_mode IN ('economy','presence')),
 left_open INTEGER NOT NULL DEFAULT 1 CHECK(left_open IN (0,1)),
 left_width INTEGER NOT NULL DEFAULT 208 CHECK(left_width BETWEEN 160 AND 320),
 right_open INTEGER NOT NULL DEFAULT 1 CHECK(right_open IN (0,1)),
 right_width INTEGER NOT NULL DEFAULT 272 CHECK(right_width BETWEEN 220 AND 360)
);
INSERT OR IGNORE INTO shell_settings(id) VALUES(1);
