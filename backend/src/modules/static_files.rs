//! Interface web embarquée dans l'exécutable (dossier frontend/dist).

use axum::{
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "../frontend/dist/"]
// Fichiers compilés de l'interface (npm run build) inclus dans le binaire à la compilation.
struct Assets;

// Sert un fichier embarqué ; repli sur index.html pour les routes de l'application monopage.
pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    // Une route API inconnue ne doit pas renvoyer l'interface.
    if path.starts_with("api/") {
        return (StatusCode::NOT_FOUND, "Route inconnue").into_response();
    }
    let file = if path.is_empty() { "index.html" } else { path };
    match Assets::get(file) {
        Some(content) => {
            let mime = mime_guess::from_path(file).first_or_octet_stream();
            // Les fichiers sous assets/ ont un nom haché par Vite : cache long ; index.html jamais mis en cache.
            let cache = if file.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
            ([(header::CONTENT_TYPE, mime.as_ref().to_string()), (header::CACHE_CONTROL, cache.to_string())], content.data)
                .into_response()
        }
        // Application monopage : les routes inconnues renvoient index.html.
        None => match Assets::get("index.html") {
            Some(index) => ([(header::CONTENT_TYPE, "text/html; charset=utf-8"), (header::CACHE_CONTROL, "no-cache")], index.data).into_response(),
            None => (StatusCode::NOT_FOUND, "Interface non compilée").into_response(),
        },
    }
}
