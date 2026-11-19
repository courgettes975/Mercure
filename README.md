# Mercure

Gestion d'activité de formation (anciennement « ERP Formation »).

Application de gestion locale (contacts, missions de formation, feuilles de temps, facturation).
Un seul exécutable, une base SQLite, aucune installation requise pour l'utilisateur final.

## Lancer l'application

1. Double-cliquer sur `app\mercure.exe`.
2. Le navigateur s'ouvre sur http://localhost:8080.
3. À la première ouverture : créer le compte administrateur.
4. Pour arrêter : fermer la fenêtre noire (console).

Options : `mercure.exe --port 8081`, `mercure.exe --data D:\Autre\Dossier`, `mercure.exe --sans-navigateur`,
`mercure.exe --reseau` (accès depuis un autre poste du réseau local, désactivé par défaut).

## Utiliser Mercure depuis plusieurs PC (mêmes données)

1. Placez le dossier de données sur un emplacement vu par les deux PC : disque réseau (`\\NAS\partage\erp`),
   lecteur partagé Windows ou clé USB.
2. Sur le premier PC : Paramètres > Stockage, indiquez ce dossier, cochez « dossier partagé », laissez
   « copier les données actuelles » coché, enregistrez, puis fermez et relancez Mercure.
3. Sur le second PC : copiez `mercure.exe` (et, pour aller plus vite, `mercure.config.json`) dans un dossier, lancez-le.
   Si vous n'avez pas copié `mercure.config.json`, indiquez le même dossier dans Paramètres > Stockage, sans copie.

Le chemin est mémorisé dans `mercure.config.json` à côté de `mercure.exe` (modifiable à la main) ; l'option `--data` prime.

Règles de prudence : un seul PC ouvre Mercure à la fois ; pas de dossier synchronisé (OneDrive, Dropbox, Google Drive)
pendant l'utilisation ; vérifiez les sauvegardes automatiques dans `backups`.

Autre solution : lancer `mercure.exe --reseau` sur un PC et ouvrir `http://<nom-ou-ip-du-pc>:8080` depuis l'autre
(un seul programme, aucune contrainte sur la base ; Mercure doit rester lancé sur le premier PC ; à réserver à un réseau local de confiance).

## Données et sauvegardes

Tout est dans le dossier `data` à côté de `mercure.exe` :

| Élément | Emplacement |
|---|---|
| Base de données | `data\erp.db` (+ fichiers `erp.db-wal` et `erp.db-shm` pendant l'exécution) |
| Sauvegardes | `data\backups\` (auto : 1 par jour, 30 conservées ; manuelles : illimitées) |
| Documents, logo | `data\documents\` |

Sauvegarde : Paramètres > Sauvegardes > "Sauvegarder maintenant". Copier aussi régulièrement le dossier
`data\backups` sur un autre support.

Restauration : arrêter Mercure, renommer `erp.db` (garder l'ancien), copier la sauvegarde choisie à la place
sous le nom `erp.db`, supprimer `erp.db-wal` et `erp.db-shm` s'ils existent, relancer.

## Développement

Prérequis : Rust (https://rustup.rs) et Node.js 22+.

```
cd frontend
npm install
npm run dev          # interface sur http://localhost:5173 (redirige /api vers 8080)

cd backend
cargo run -- --sans-navigateur --data ..\data-dev
```

Tests :
```
cd backend  && cargo test
cd frontend && npm test
```

Compilation de l'exécutable Windows : `powershell -ExecutionPolicy Bypass -File build.ps1`
(produit `app\mercure.exe`).

## Structure

```
backend/        Rust (Axum + SQLx + SQLite)
  migrations/   schéma SQL versionné, appliqué automatiquement au démarrage
  src/domain/   règles pures : montants (centimes), rôles, validations
  src/modules/  services par module (utilisateurs, paramètres, journal, séquences, sauvegardes...)
  tests/        tests d'intégration de l'API
frontend/       React + TypeScript (aucune règle métier)
  src/components/  bibliothèque commune (tableau, champs, modale, badges, pagination...)
  src/modules/     écrans par module
docs/           registre des écrans, modèle de domaine, plan, fiches, captures
app/            exécutable compilé
```

## Modules
Contacts (import Odoo inclus), Projets, Missions de formation (sessions, participants, frais, documents, pédagogie),
Feuilles de temps (grille et liste), Facturation (factures, avoirs, paiements, PDF), Tableau de bord, Paramètres.

## Règles techniques

- Montants en centimes entiers, jamais de nombres à virgule flottante.
- Calculs, numérotation et permissions contrôlés côté serveur.
- Opérations sensibles inscrites dans un journal non modifiable.

## Accès depuis les autres PC du réseau

Deux solutions, réglables dans Paramètres > Stockage et réseau (ADMIN) :

1. **Dossier de données partagé** (NAS, OneDrive, clé USB) : un seul PC utilise Mercure à la fois.
2. **Accès réseau** : Mercure tourne sur un PC, les autres l'ouvrent dans leur navigateur à `http://<nom-du-pc>:8080`.
   Cochez "Autoriser les autres PC du réseau local", enregistrez, fermez puis relancez `mercure.exe`.
   Au premier lancement Windows demande d'autoriser `mercure.exe` dans le pare-feu : acceptez (réseau privé).
   Equivalent en ligne de commande : `mercure.exe --reseau --port 8080`.
   A réserver à un réseau local de confiance : la connexion n'est pas chiffrée.
