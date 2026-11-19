//! Séquences de numérotation gérées côté serveur.
//! À appeler dans une transaction ouverte avec `BEGIN IMMEDIATE` :
//! SQLite sérialise alors les écritures, ce qui exclut tout doublon.

use sqlx::SqliteConnection;

use crate::error::AppResult;

/// Réserve la prochaine valeur de la séquence `key` et la renvoie.
pub async fn next_value(conn: &mut SqliteConnection, key: &str) -> AppResult<i64> {
    // Une seule instruction atomique : création de la clé à 2 (on rend 1) ou incrément, puis lecture.
    let value: i64 = sqlx::query_scalar(
        "INSERT INTO sequences (key, next_value) VALUES (?1, 2)
         ON CONFLICT(key) DO UPDATE SET next_value = next_value + 1
         RETURNING next_value - 1",
    )
    .bind(key)
    .fetch_one(conn)
    .await?;
    Ok(value)
}

/// Exemple : format_reference("FAC", 2026, 1, 6) -> "FAC-2026-000001"
pub fn format_reference(prefix: &str, year: i32, value: i64, width: usize) -> String {
    format!("{prefix}-{year}-{value:0width$}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(format_reference("FAC", 2026, 1, 6), "FAC-2026-000001");
        assert_eq!(format_reference("MF", 2026, 12, 4), "MF-2026-0012");
    }
}
