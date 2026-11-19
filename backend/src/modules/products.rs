//! Liste de référence des produits / prestations (Paramètres > Produits).
//! Un produit pré-remplit une ligne de facture : libellé, unité, prix unitaire HT et taux de TVA.

use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    auth::CurrentUser,
    domain::{roles::Permission, validation::normalize_spaces},
    error::{AppError, AppResult},
    modules::audit,
    state::AppState,
};

/// Produit tel que renvoyé à l'interface.
#[derive(Serialize, Deserialize, sqlx::FromRow, Clone, Debug)]
pub struct Product {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub unit: String,
    /// Prix unitaire HT en centimes.
    pub unit_price: i64,
    /// Taux de TVA en points de base (2000 = 20 %).
    pub vat_rate_bp: i64,
    pub active: bool,
    pub position: i64,
}

const SELECT: &str = "SELECT id, name, description, unit, unit_price, vat_rate_bp, active, position FROM products";

/// GET /products : tous les produits (actifs et archivés), dans l'ordre d'affichage.
pub async fn list(State(st): State<AppState>, _user: CurrentUser) -> AppResult<Json<Vec<Product>>> {
    let rows = sqlx::query_as::<_, Product>(sqlx::AssertSqlSafe(format!("{SELECT} ORDER BY position, name COLLATE NOCASE")))
        .fetch_all(&st.pool)
        .await?;
    Ok(Json(rows))
}

/// Création ou modification partielle : seuls les champs présents changent.
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ProductInput {
    pub name: Option<String>,
    pub description: Option<String>,
    pub unit: Option<String>,
    pub unit_price: Option<i64>,
    pub vat_rate_bp: Option<i64>,
    pub active: Option<bool>,
    pub position: Option<i64>,
}

fn conflict_msg(e: sqlx::Error) -> AppError {
    match AppError::from(e) {
        AppError::Conflict(_) => AppError::Conflict("Un produit porte déjà ce nom".into()),
        other => other,
    }
}

/// Contrôles communs (après fusion avec les valeurs existantes).
fn check(p: &Product) -> AppResult<()> {
    if p.name.is_empty() {
        return Err(AppError::Validation("Le nom du produit est obligatoire".into()));
    }
    if p.unit_price < 0 {
        return Err(AppError::Validation("Le prix unitaire doit être positif ou nul".into()));
    }
    if !(0..=10_000).contains(&p.vat_rate_bp) {
        return Err(AppError::Validation("Taux de TVA invalide (0 à 100 %)".into()));
    }
    Ok(())
}

/// POST /products
pub async fn create(State(st): State<AppState>, user: CurrentUser, Json(i): Json<ProductInput>) -> AppResult<Json<Product>> {
    user.require(Permission::ManageReferenceLists)?;
    let p = Product {
        id: 0,
        name: normalize_spaces(i.name.as_deref().unwrap_or("")),
        description: i.description.unwrap_or_default().trim().to_string(),
        unit: normalize_spaces(i.unit.as_deref().unwrap_or("h")),
        unit_price: i.unit_price.unwrap_or(0),
        vat_rate_bp: i.vat_rate_bp.unwrap_or(0),
        active: i.active.unwrap_or(true),
        position: i.position.unwrap_or(0),
    };
    check(&p)?;
    let mut tx = st.pool.begin().await?;
    // Position par défaut : à la fin de la liste.
    let position = if p.position == 0 {
        sqlx::query_scalar::<_, i64>("SELECT COALESCE(MAX(position), 0) + 1 FROM products").fetch_one(&mut *tx).await?
    } else {
        p.position
    };
    let row = sqlx::query_as::<_, Product>(
        "INSERT INTO products (name, description, unit, unit_price, vat_rate_bp, active, position) VALUES (?, ?, ?, ?, ?, ?, ?)
         RETURNING id, name, description, unit, unit_price, vat_rate_bp, active, position",
    )
    .bind(&p.name).bind(&p.description).bind(&p.unit).bind(p.unit_price).bind(p.vat_rate_bp).bind(p.active).bind(position)
    .fetch_one(&mut *tx)
    .await
    .map_err(conflict_msg)?;
    audit::record(&mut tx, Some(user.id), "CREATION", "product", Some(row.id.to_string()), Some(json!(row))).await?;
    tx.commit().await?;
    Ok(Json(row))
}

/// PATCH /products/{id}
pub async fn update(State(st): State<AppState>, user: CurrentUser, Path(id): Path<i64>, Json(i): Json<ProductInput>) -> AppResult<Json<Product>> {
    user.require(Permission::ManageReferenceLists)?;
    let mut tx = st.pool.begin().await?;
    let before = sqlx::query_as::<_, Product>(sqlx::AssertSqlSafe(format!("{SELECT} WHERE id = ?")))
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::NotFound("Produit introuvable".into()))?;
    let p = Product {
        id,
        name: i.name.map(|n| normalize_spaces(&n)).unwrap_or(before.name.clone()),
        description: i.description.map(|d| d.trim().to_string()).unwrap_or(before.description.clone()),
        unit: i.unit.map(|u| normalize_spaces(&u)).unwrap_or(before.unit.clone()),
        unit_price: i.unit_price.unwrap_or(before.unit_price),
        vat_rate_bp: i.vat_rate_bp.unwrap_or(before.vat_rate_bp),
        active: i.active.unwrap_or(before.active),
        position: i.position.unwrap_or(before.position),
    };
    check(&p)?;
    let after = sqlx::query_as::<_, Product>(
        "UPDATE products SET name = ?, description = ?, unit = ?, unit_price = ?, vat_rate_bp = ?, active = ?, position = ? WHERE id = ?
         RETURNING id, name, description, unit, unit_price, vat_rate_bp, active, position",
    )
    .bind(&p.name).bind(&p.description).bind(&p.unit).bind(p.unit_price).bind(p.vat_rate_bp).bind(p.active).bind(p.position).bind(id)
    .fetch_one(&mut *tx)
    .await
    .map_err(conflict_msg)?;
    audit::record(&mut tx, Some(user.id), "MODIFICATION", "product", Some(id.to_string()), Some(audit::diff(&json!(before), &json!(after)))).await?;
    tx.commit().await?;
    Ok(Json(after))
}
