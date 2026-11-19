//! Point d'entrée de l'exécutable Mercure : mercure.exe [--data DOSSIER] [--port 8080] [--reseau] [--sans-navigateur]
//!
//! Rôle de ce fichier :
//! 1. lire la configuration locale (mercure.config.json) et les options de la ligne de commande ;
//! 2. ouvrir (ou créer) le dossier de données et la base SQLite, appliquer les migrations ;
//! 3. lancer la sauvegarde quotidienne, puis le serveur HTTP qui sert l'API et l'interface ;
//! 4. ouvrir le navigateur de l'utilisateur sur l'adresse locale.

use std::{net::SocketAddr, path::PathBuf};

// Tout le métier vit dans la bibliothèque `erp` (lib.rs) ; ce binaire ne fait que l'assembler.
use erp::{auth::LoginGuard, build_router, config, db, modules::backup, state::{AppState, DataPaths}};

/// Options effectives après fusion configuration + ligne de commande.
struct Args {
    /// Dossier de données imposé par `--data` (prime sur la configuration).
    data: Option<PathBuf>,
    /// Port d'écoute HTTP.
    port: u16,
    /// Écouter sur toutes les interfaces (accès depuis les autres PC du réseau local).
    lan: bool,
    /// Ouvrir automatiquement le navigateur au démarrage.
    open_browser: bool,
}

/// Lit les arguments de la ligne de commande ; les valeurs de la configuration servent de défauts.
fn parse_args(cfg: &config::Config) -> Args {
    // Défauts : port et accès réseau pris dans mercure.config.json, sinon 8080 / local seulement.
    let mut args = Args { data: None, port: cfg.port.unwrap_or(8080), lan: cfg.lan.unwrap_or(false), open_browser: true };
    let mut it = std::env::args().skip(1); // on saute le nom de l'exécutable
    while let Some(a) = it.next() {
        match a.as_str() {
            // --data DOSSIER : l'argument suivant est le chemin.
            "--data" => {
                if let Some(v) = it.next() {
                    args.data = Some(PathBuf::from(v));
                }
            }
            // --port N : ignoré silencieusement si la valeur n'est pas un entier valide.
            "--port" => {
                if let Some(v) = it.next().and_then(|v| v.parse().ok()) {
                    args.port = v;
                }
            }
            "--reseau" => args.lan = true,
            "--sans-navigateur" => args.open_browser = false,
            // Aide : affiche la syntaxe et quitte sans démarrer.
            "--aide" | "--help" | "-h" => {
                println!("mercure [--data DOSSIER] [--port 8080] [--reseau] [--sans-navigateur]");
                std::process::exit(0);
            }
            // Option inconnue : on prévient mais on démarre quand même.
            other => eprintln!("option ignorée : {other}"),
        }
    }
    args
}

/// Laisse le temps de lire l'erreur quand l'exécutable a été lancé par double-clic
/// (sinon la fenêtre noire se ferme immédiatement et le message est perdu).
fn wait_and_exit() -> ! {
    eprintln!("Appuyez sur Entrée pour fermer.");
    let _ = std::io::stdin().read_line(&mut String::new());
    std::process::exit(1)
}

#[tokio::main]
async fn main() {
    // Journalisation technique sur la console (niveau INFO par défaut).
    tracing_subscriber::fmt().with_target(false).init();

    // 1. Configuration et arguments.
    let cfg = config::load();
    let args = parse_args(&cfg);
    let data_dir = config::resolve_data_dir(args.data.clone(), &cfg);
    // Un dossier réseau ou synchronisé impose le journal SQLite classique (voir db.rs).
    let shared = cfg.shared || config::looks_shared(&data_dir);

    // 2. Dossier de données (base, documents, sauvegardes) : création si absent.
    let paths = DataPaths::new(data_dir.clone());
    if let Err(e) = paths.ensure() {
        eprintln!("Impossible d'ouvrir le dossier de données {} : {e}", data_dir.display());
        eprintln!("Vérifiez que le lecteur réseau est connecté, ou corrigez le chemin dans {}", config::config_path().display());
        wait_and_exit();
    }
    // Ouverture de la base + migrations ; toute erreur ici est bloquante.
    let pool = match db::open_with(&paths.db_file, shared).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Impossible d'ouvrir la base {} : {e}", paths.db_file.display());
            wait_and_exit();
        }
    };
    // État partagé par toutes les requêtes HTTP (voir state.rs).
    let state = AppState { pool, paths, secure_cookies: false, login_guard: LoginGuard::default(), shared_storage: shared, lan: args.lan, port: args.port };

    // 3. Sauvegarde automatique : une fois au démarrage si celle du jour manque, puis tâche de fond.
    match backup::daily_if_needed(&state).await {
        Ok(Some(name)) => tracing::info!("sauvegarde automatique : {name}"),
        Ok(None) => {}
        Err(e) => tracing::error!("échec de la sauvegarde automatique : {e:?}"),
    }
    backup::spawn_daily_task(state.clone());

    // Adresse d'écoute : 127.0.0.1 (ce PC seulement) ou 0.0.0.0 (tout le réseau local) avec --reseau.
    let ip = if args.lan { [0, 0, 0, 0] } else { [127, 0, 0, 1] };
    let addr = SocketAddr::from((ip, args.port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        // Port déjà pris : l'ERP tourne probablement déjà, on ouvre simplement le navigateur dessus.
        Err(e) => {
            eprintln!("Port {} indisponible ({e}). Mercure est peut-être déjà lancé.", args.port);
            let _ = webbrowser::open(&format!("http://localhost:{}", args.port));
            std::process::exit(1);
        }
    };

    // Messages d'accueil dans la console.
    let url = format!("http://localhost:{}", args.port);
    println!("Mercure démarré : {url}");
    if args.lan {
        println!("Accès réseau activé : les autres PC peuvent ouvrir http://<nom-de-ce-pc>:{}", args.port);
    }
    println!("Données : {}{}", state.paths.root.display(), if shared { " (dossier partagé)" } else { "" });
    println!("Fermez cette fenêtre pour arrêter Mercure.");
    // 4. Ouverture du navigateur (sauf --sans-navigateur, utile pour les tests ou un service).
    if args.open_browser {
        let _ = webbrowser::open(&url);
    }

    // Serveur HTTP : s'arrête proprement sur Ctrl+C / fermeture de la fenêtre.
    let app = build_router(state.clone());
    let server = axum::serve(listener, app).with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    });
    if let Err(e) = server.await {
        eprintln!("Erreur du serveur : {e}");
    }
    // Fermeture propre des connexions SQLite (écriture du journal).
    state.pool.close().await;
}
