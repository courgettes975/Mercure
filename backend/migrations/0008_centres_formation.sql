-- Liste de référence des centres de formation (contacts société/organisme marqués)
ALTER TABLE partners ADD COLUMN is_training_center INTEGER NOT NULL DEFAULT 0 CHECK (is_training_center IN (0, 1));
UPDATE partners SET is_training_center = 1 WHERE id IN (SELECT center_id FROM missions WHERE center_id IS NOT NULL);
