//! Journal des opérations sensibles (ajout seul).

use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqliteConnection;

use crate::{
    auth::CurrentUser,
    domain::roles::Permission,
    error::{AppError, AppResult},
    state::AppState,
};

// Écrit une ligne de journal. `changes` reçoit un JSON libre (valeurs créées, diff, paramètres...).
// La table est protégée par des déclencheurs SQL : aucune mise à jour ni suppression possible.
pub async fn record(
    conn: &mut SqliteConnection,
    user_id: Option<i64>,
    action: &str,
    entity_type: &str,
    entity_id: Option<String>,
    changes: Option<Value>,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO audit_log (user_id, action, entity_type, entity_id, changes) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(user_id)
    .bind(action)
    .bind(entity_type)
    .bind(entity_id)
    .bind(changes.map(|c| c.to_string()))
    .execute(conn)
    .await?;
    Ok(())
}

/// Différences champ par champ entre deux objets JSON (ancienne / nouvelle valeur).
pub fn diff(before: &Value, after: &Value) -> Value {
    let mut out = serde_json::Map::new();
    if let (Value::Object(b), Value::Object(a)) = (before, after) {
        for (k, new_v) in a {
            let old_v = b.get(k).unwrap_or(&Value::Null);
            if old_v != new_v {
                out.insert(k.clone(), serde_json::json!({ "old": old_v, "new": new_v }));
            }
        }
    }
    Value::Object(out)
}

#[derive(Deserialize)]
// Paramètres de la liste : pagination et filtre sur le type d'entité.
pub struct AuditQuery {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    pub entity_type: Option<String>,
}

#[derive(Serialize, sqlx::FromRow)]
// Une ligne du journal, avec le nom de l'utilisateur résolu.
pub struct AuditEntry {
    pub id: i64,
    pub user_id: Option<i64>,
    pub user_name: Option<String>,
    pub action: String,
    pub entity_type: String,
    pub entity_id: Option<String>,
    pub changes: Option<String>,
    pub created_at: String,
}

#[derive(Serialize)]
// Réponse paginée générique, réutilisée par d'autres modules.
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
}

// Consultation du journal (droit ViewAudit), du plus récent au plus ancien.
pub async fn list(
    State(st): State<AppState>,
    user: CurrentUser,
    Query(q): Query<AuditQuery>,
) -> AppResult<Json<Page<AuditEntry>>> {
    user.require(Permission::ViewAudit)?;
    // Bornes de pagination : 1 à 200 lignes par page.
    let per_page = q.per_page.unwrap_or(50).clamp(1, 200);
    let page = q.page.unwrap_or(1).max(1);
    let filter = q.entity_type.filter(|s| !s.is_empty());
    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_log WHERE (?1 IS NULL OR entity_type = ?1)",
    )
    .bind(&filter)
    .fetch_one(&st.pool)
    .await?;
    let items = sqlx::query_as::<_, AuditEntry>(
        "SELECT a.id, a.user_id, u.display_name AS user_name, a.action, a.entity_type, a.entity_id,
                a.changes, a.created_at
         FROM audit_log a LEFT JOIN users u ON u.id = a.user_id
         WHERE (?1 IS NULL OR a.entity_type = ?1)
         ORDER BY a.id DESC LIMIT ?2 OFFSET ?3",
    )
    .bind(&filter)
    .bind(per_page)
    .bind((page - 1) * per_page)
    .fetch_all(&st.pool)
    .await?;
    // Page au-delà de la fin : 404 plutôt qu'une page vide trompeuse.
    if page > 1 && items.is_empty() && total > 0 {
        return Err(AppError::NotFound("Page inexistante".into()));
    }
    Ok(Json(Page { items, total, page, per_page }))
}
