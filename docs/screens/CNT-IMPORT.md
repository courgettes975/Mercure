# CNT-003 - Import des contacts Odoo

Source : export Odoo "Contact (res.partner)" en .xlsx (119 lignes), colonnes : Avatar, Nom d'affichage, E-mail, Téléphone,
Activités, Pays, Stats, Date du dernier rappel. Les colonnes Rue, Code postal, Ville, SIRET, TVA, Étiquettes,
Est une société sont reconnues si elles sont présentes dans un export plus complet.

## Correspondance des champs
| Odoo | ERP |
|---|---|
| "SOCIÉTÉ, Personne" | personne rattachée à la société (créée si absente du fichier) |
| Nom en MAJUSCULES + prénom | nom / prénom |
| E-mail (plusieurs, ": " parasite) | premier email valide ; les autres en note |
| Téléphone "a / b", 9 chiffres | numéros séparés, zéro initial rétabli, 06/07 -> mobile |
| Pays | pays (France par défaut) |
| Stats (factures, commandes, abonnements) | note "Odoo : 24 factures..." + rôle Client |
| Avatar, Activités, Dernier rappel | ignorés |

## Règles (hypothèses)
- Société : nom utilisé comme rattachement, ou mot-clé (SAS, SARL, association, campus, agence, groupe...).
  ORGANISME si le nom évoque la formation, ENTREPRISE sinon.
- Stagiaire : personne rattachée à une société cliente, avec un email hors du domaine de la société et un NOM identifiable.
- "ARLIGUY Hanna - Utilisation d'un logiciel ..." : nom seul conservé, intitulé en note, étiquette "Fiche formation Odoo"
  (à transformer en mission).
- Étiquettes de revue : "Import Odoo" (tous), "Doublon possible" (même email ou même nom), "À compléter" (nom = email).
- Référence d'import "odoo:<nom d'affichage>" unique : réimporter le même fichier ne crée rien.
- Import en une transaction : tout ou rien. Aucune fusion ni suppression automatique.

## Résultat sur le fichier fourni
119 contacts (16 sociétés, 103 personnes), 29 doublons possibles, 39 "à compléter", 1 fiche formation.
