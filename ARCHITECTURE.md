# Architecture de Magic Deck

## Objectif et contraintes

Magic Deck est une application web locale qui transforme les données présentes dans le journal détaillé de MTG Arena en vues et exports exploitables. Elle cible Ubuntu avec MTGA installé par Steam/Proton.

Les décisions structurantes sont les suivantes :

- un seul exécutable Rust ;
- aucune authentification auprès de MTGA ; l'envoi à Gemini est limité aux analyses explicitement demandées ;
- aucune chaîne de compilation frontend ;
- une synchronisation explicite depuis `Player.log` et le processus MTGA local ;
- un snapshot en mémoire, un cache de collection sur disque et un historique SQLite des analyses.

Ce périmètre réduit les prérequis, les risques sur les données utilisateur et le coût d'exploitation d'une application destinée à `127.0.0.1`.

## Vue d'ensemble

```text
Player.log ── lecture en flux ──> Parser ──> Snapshot en mémoire
MTGA.exe ─── lecture seule RAM ──> Collection ───────┤
Navigateur <── HTML/JSON/CSV/TXT ── Axum routes ────┘
                                      │
                                      └── ~/.config/magic-deck/config.json
```

Au démarrage, l'application charge la configuration et tente une première synchronisation. Un échec de lecture n'empêche pas le serveur de démarrer : l'utilisateur peut corriger le chemin depuis l'interface. Les synchronisations suivantes sont déclenchées par `POST /api/sync`.

## Découpage des modules

| Module | Responsabilité | Raison du découpage |
|---|---|---|
| `main.rs` | Initialisation, routes, écoute réseau et arrêt propre | Garder le point d'entrée limité à l'assemblage de l'application |
| `config.rs` | Valeur par défaut, chargement et sauvegarde atomique | Isoler les effets de bord liés au système de fichiers |
| `parser.rs` | Lecture du journal et construction d'un `Snapshot` | Séparer le format instable de MTGA du transport HTTP |
| `memory_collection.rs` | Extraction en lecture seule des quantités possédées depuis `MTGA.exe` | Compenser la suppression de la collection dans les journaux MTGA actuels |
| `collection_cache.rs` | Persistance JSON atomique de la dernière collection valide | Garder l'inventaire disponible lorsque MTGA est fermé |
| `ai_coach.rs` | Client Gemini, schéma de sortie et validation métier | Isoler l'API externe et refuser toute suggestion hors collection |
| `analysis_store.rs` | Persistance SQLite des rapports et snapshots | Rendre chaque analyse datée et reproductible malgré les synchronisations futures |
| `model.rs` | Types sérialisés de cartes, decks, statut et configuration | Partager un contrat unique entre parser, routes et exports |
| `export.rs` | Production des formats Arena, JSON et CSV | Tester indépendamment les règles de représentation |
| `routes.rs` | État partagé, handlers HTTP, filtrage et erreurs API | Centraliser la frontière web sans la mélanger au parsing |
| `coach_context.rs` | Lecture bornée des capacités locales en anglais | Réduire les inventions sans envoyer toute la base MTGA |
| `src/prompts/deck_coach.txt` | Instruction système du coach | Modifier et versionner le comportement sans le mélanger au transport HTTP |
| `web/index.html`, `app.css` | Coquille et système visuel | Séparer structure et présentation, toujours embarquées dans le binaire |
| `web/app.js` | Navigation, API et vues générales | Conserver un point d'assemblage léger sans framework frontend |
| `web/collection.js`, `coach.js`, `preview.js` | Galerie, atelier de combos, aperçu partagé | Isoler les interactions métier pour éviter un document monolithique |

## Backend Axum et Tokio

Axum a été retenu pour son intégration directe avec Tokio, ses extracteurs typés et son faible niveau d'abstraction. Les handlers retournent des types sérialisables ou une erreur API uniforme au format JSON.

Tokio fournit :

- le serveur HTTP asynchrone ;
- les verrous `RwLock` de l'état partagé ;
- `spawn_blocking` pour la lecture et le parsing du journal ;
- la gestion de `SIGINT` et `SIGTERM`.

Le parsing est synchrone et potentiellement long. L'exécuter dans `spawn_blocking` évite de bloquer les threads chargés des requêtes HTTP. Le serveur écoute uniquement sur `127.0.0.1`, afin de ne pas exposer par défaut les informations locales sur le réseau.

`tower-http` ajoute la trace des requêtes et la compression gzip. `tracing` rend le niveau de logs configurable avec `RUST_LOG`.

## État et cache

`AppState` contient deux valeurs partagées :

- `Settings`, protégée par un `RwLock` ;
- le dernier `Snapshot` valide, également protégé par un `RwLock`.

Un snapshot contient la collection, les decks, les wildcards, les horodatages et les avertissements. Une nouvelle synchronisation remplace le snapshot en une seule écriture seulement après un parsing réussi. Ainsi, une erreur temporaire ne détruit pas les données déjà chargées.

Magic Deck ne crée pas de base pour le parsing : les volumes courants tiennent facilement en mémoire. `Player.log` fournit les decks et les wildcards ; le processus MTGA fournit la collection courante. Après chaque synchronisation réussie, `collection_cache.rs` écrit un snapshot JSON atomique dans le dossier de données local. Au démarrage ou lorsque MTGA est fermé, ce snapshot est rechargé comme source de repli. La base SQLite de cartes installée par MTGA est ouverte en lecture seule pour résoudre les identifiants Arena. Une petite base SQLite séparée conserve les rapports IA et leurs snapshots, afin de rendre l'historique reproductible.

## Parser MTGA

Le format de `Player.log` n'est pas une API publique stable. Le parser privilégie donc une stratégie tolérante plutôt qu'un schéma JSON unique :

1. lecture ligne par ligne avec `BufReader`, pour ne pas charger le journal entier ;
2. détection du contexte par les marqueurs MTGA connus (`GetPlayerCards`, `GetDeckLists`, `UpsertDeck`, inventaire, etc.) ;
3. reconstruction des blocs JSON multilignes par équilibrage des accolades et crochets, en tenant compte des chaînes échappées ;
4. décodage avec `serde_json` ;
5. parcours récursif des enveloppes et des payloads JSON encodés sous forme de chaîne ;
6. normalisation insensible à la casse des noms de champs ;
7. conservation de la collection la plus récente et fusion des decks par identifiant ;
8. priorité donnée à `DecksInternal`, qui représente le deck personnel courant, sur les anciens snapshots `CourseDeck` liés à des événements.
9. exclusion du catalogue `PreconDecks` et des entrées `IsNetDeck`, qui ne sont pas des decks personnels.

Plusieurs représentations de listes de cartes sont acceptées : objet `{id: quantité}`, tableau plat `[id, quantité, ...]`, paires ou objets contenant `cardId`/`grpId`. La profondeur de parcours est limitée à 12 pour éviter une récursion non bornée sur une entrée hostile ou anormale.

Le journal fournit souvent uniquement les identifiants numériques Arena. Le modèle conserve toujours ces identifiants exacts, puis `card_database.rs` recherche automatiquement le fichier `Raw_CardDatabase_*.mtga` de l'installation Steam. Il en lit en anglais les noms, couleurs, types, sets et numéros de collection, car le format d'import Arena utilise ces noms. La base est ouverte en lecture seule et le fichier le plus récent est choisi. Si elle est absente, l'interface retombe sur `Arena card #ID` sans inventer de correspondance.

L'interface enrichit les cartes avec une miniature Scryfall chargée à la demande : URL de l'impression exacte (`set` + `collector number`) lorsque ces métadonnées existent, sinon recherche par nom exact. Les images sont `lazy`, masquées si le réseau ou la correspondance échoue, afin de préserver le fonctionnement local hors ligne. Les icônes de navigation restent inline/CSS et ne dépendent pas d'un téléchargement de police ou d'un asset propriétaire.

Lors d'une analyse, `candidate_selector.rs` applique le format du deck avant le scoring. `Standard` et `Alchemy` utilisent une allowlist de codes d'extensions, remplaçable par `MAGIC_DECK_STANDARD_SETS` pour suivre les rotations. Les formats Eternal et Limited ne sont pas filtrés par bannissement, car la base SQLite MTGA ne fournit pas ces listes ; le serveur conserve néanmoins la validation stricte des quantités possédées.

Les noms techniques de decks commençant par `?=?Loc/` sont résolus dans `Raw_ClientLocalization_*.mtga`. La langue est déduite des noms déjà localisés présents dans le journal, avec repli sur la locale du système puis sur l'anglais.

## Collection courante sous Linux

Les clients MTGA actuels ne journalisent plus la réponse historique `GetPlayerCardsV3`. Ouvrir l'écran Collection ne peut donc pas rendre les quantités disponibles dans `Player.log`. Lorsque le journal ne contient aucun ancien snapshot, `memory_collection.rs` détecte la plateforme : sous Linux, il localise le processus Wine `MTGA.exe` et lit uniquement ses régions privées `rw-p` via `/proc/<pid>/mem`; sous Windows, il utilise `OpenProcess`, `VirtualQueryEx` et `ReadProcessMemory` sur `MTGA.exe`. Dans les deux cas, il cherche les structures `(identifiant Arena, quantité)` du runtime Mono. Un candidat n'est accepté que si au moins 90 % de ses identifiants existent dans la base SQLite locale, afin d'écarter les faux positifs.

Cette solution reste entièrement locale, ne modifie pas le processus et n'utilise ni identifiants ni API réseau MTGA. Elle exige que le jeu soit lancé par le même utilisateur et que la politique Linux autorise cette lecture. En cas d'échec, le snapshot reste valide pour les decks et expose un avertissement explicite.

## API HTTP

| Méthode | Route | Fonction |
|---|---|---|
| `GET` | `/` | Interface embarquée |
| `GET` | `/health` | Sonde de disponibilité |
| `GET` | `/api/status` | État du fichier, statistiques et avertissements |
| `POST` | `/api/sync` | Nouvelle lecture complète du journal |
| `GET`, `PUT` | `/api/settings` | Lecture et modification du chemin du journal et de la clé Gemini |
| `GET` | `/api/decks` | Liste des decks |
| `GET` | `/api/decks/{id}` | Détail d'un deck |
| `GET` | `/api/decks/{id}/export?format=…` | Export Arena, JSON ou CSV |
| `GET` | `/api/collection` | Collection avec filtres optionnels |
| `GET` | `/api/collection/export?format=…` | Export de la collection |
| `POST` | `/api/analyze-deck` | Analyse Gemini d'un deck avec la collection courante |
| `GET` | `/api/decks/{id}/analyses` | Historique daté des analyses du deck |
| `GET` | `/api/analyses/{id}` | Consultation d'un rapport enregistré |

Les erreurs attendues, comme un chemin invalide ou un deck absent, produisent un code HTTP adapté et `{ "error": "…" }`. Les erreurs internes sont journalisées côté serveur sans exposer leurs détails au navigateur.

## Interface web

L'interface et ses fichiers CSS/JS sont embarqués à la compilation avec `include_str!`. `/assets/{name}` ne sert qu'une liste explicite de fichiers connus, sans accès arbitraire au disque. Ce choix conserve le binaire unique tout en séparant les responsabilités. Les scripts classiques différés gardent quelques helpers partagés pour préserver les actions existantes ; ce n'est pas encore une architecture à composants isolés.

La collection garde son état de filtres séparé du rendu des résultats : la saisie ne perd plus son focus à chaque caractère. La pagination limite le DOM à 36 cartes ; elle est locale (le snapshot complet est chargé). Les favoris sont un ensemble d'identifiants Arena dans `localStorage`, un choix léger pour une préférence visuelle propre au navigateur, distinct de l'inventaire possédé. Les options avancées sont repliées sur mobile pour laisser de la place aux illustrations.

Un unique `<dialog>` natif affiche les cartes au clic, y compris dans les decks et rapports. Il fournit confinement et restauration du focus, fermeture par Échap et animation inverse sans dépendre de Motion. Les animations respectent `prefers-reduced-motion`. L'atelier distingue séquences jouables, résultat attendu et limites ; il ne transforme pas un conseil IA en preuve mécanique. Les boutons permettent de parcourir les étapes sans masquer les conditions nécessaires.

Tailwind CSS est chargé par CDN conformément au choix d'un MVP sans build frontend. Le JavaScript natif utilise `fetch`, maintient un petit état côté navigateur et échappe les données avant insertion dans le DOM. Les vues Home, Decks, Collection et Settings restent utilisables sur mobile et bureau. Une connexion Internet est nécessaire au chargement de la page pour obtenir les styles Tailwind ; les API et les données restent locales.

## Configuration

L'ordre de résolution du chemin du journal est :

1. variable `MTGA_LOG_PATH` ;
2. `~/.config/magic-deck/config.json` ;
3. chemin Steam/Proton par défaut.

La base de cartes est cherchée dans la même bibliothèque Steam, puis dans les emplacements Steam usuels. `MTGA_CARD_DB_PATH` permet de fournir explicitement son chemin. `MAGIC_DECK_LOCALE` peut forcer la langue des noms de decks, par exemple `frFR`.

Une configuration saisie dans l'interface est sauvegardée via un fichier temporaire puis renommée, afin de limiter le risque d'un JSON partiellement écrit. `MAGIC_DECK_PORT`, puis `PORT`, permettent de remplacer le port `8092`.

## Exports

- Arena : sections `Deck`, `Commander` et `Sideboard`, avec set et numéro lorsqu'ils sont connus ;
- JSON : représentation complète des structures internes ;
- CSV : colonnes stables et échappement conforme des virgules, guillemets et retours à la ligne.

Les réponses utilisent `Content-Disposition: attachment`, tandis que le bouton Arena récupère le texte et le place dans le presse-papiers.

## Coach IA et persistance

Le handler copie le deck et la collection depuis le snapshot courant avant tout appel externe, puis libère le verrou partagé. `candidate_selector.rs` réduit localement la collection à 80 cartes pertinentes (couleurs, types, terrains et cartes déjà présentes). `ai_coach.rs` envoie le deck et cette liste courte à Gemini avec une instruction système restrictive, `responseMimeType: application/json` et un `responseJsonSchema`. La sortie est plafonnée à 4 096 tokens et trois suggestions, afin d'éviter un JSON coupé en plein champ. Le modèle stable par défaut est `gemini-3.6-flash`, configurable par `GEMINI_MODEL`.

`coach_context.rs` lit la base Arena en lecture seule dans `spawn_blocking`. Les identifiants de capacités utilisent la première composante de `BaseAbilityId:variantId`, résolue via `Abilities` et `Localizations_enUS`. Mana et textes complets sont limités à 12 000 caractères, en privilégiant les cartes du deck ; les textes absents, inconnus ou dépassant le budget sont omis, jamais tronqués au milieu d'une règle. Si la base ou son schéma est indisponible, le contexte est vide et le prompt demande explicitement de ne pas inventer les capacités manquantes.

Le décodage ignore les parties de réflexion, exige `finishReason=STOP` et refuse les champs inattendus. La validation locale vérifie retraits, appartenance des ajouts à la présélection et quantités finales dans la collection complète (toutes impressions confondues par nom). Les combos ont 2 à 4 cartes distinctes fournies dans le deck ou la présélection, réellement possédées, 2 à 5 étapes et des prérequis/limites non vides. Le plafond est de trois combos et trois changements indépendants. Un rapport non conforme est rejeté et n'est pas enregistré. Ces vérifications structurelles ne simulent pas les règles de Magic et ne prouvent donc aucune boucle infinie.

Les nouveaux champs `combos` et `play_challenge` ont des valeurs par défaut Serde : aucun historique n'est supprimé et les anciens JSON restent lisibles. L'interface indique leur absence au lieu d'inventer des combos à partir d'un ancien rapport.

Avant l'appel, une empreinte SHA-256 du deck, de la collection, du modèle, de `COACH_VERSION` et du prompt est recherchée dans SQLite. Une correspondance renvoie immédiatement le rapport sans consommer de tokens. Une nouvelle instruction ne recycle donc pas un ancien rapport sans combos. Les modifications du sélecteur ou du contrat doivent incrémenter `COACH_VERSION` ; le contenu externe de la base de règles n'est pas inclus dans cette empreinte. Après validation, `analysis_store.rs` écrit le rapport, sa date, le modèle et les représentations JSON exactes du deck et de la collection. La clé Gemini peut venir de `GEMINI_API_KEY` (prioritaire) ou des Settings ; dans ce dernier cas elle est stockée localement dans le fichier de configuration protégé en mode `0600` et n'est jamais renvoyée au navigateur. Le fichier de rapports utilise WAL et un index `(deck_id, fingerprint, model)` pour un historique rapide sans conserver une connexion SQLite dans l'état asynchrone.

## Validation et évolutions

Les tests unitaires couvrent les payloads préfixés, imbriqués et multilignes, les variantes de decks, la sélection du dernier snapshot, les filtres et les exports. `cargo clippy --all-targets -- -D warnings` complète la validation statique.

Évolutions naturelles après le MVP :

- surveillance incrémentale du journal ;
- moteur de règles ou sources vérifiées pour certifier les interactions (hors périmètre du coach génératif) ;
- pagination serveur pour de très grandes collections ;
- copie locale de Tailwind ou CSS compilé pour un fonctionnement entièrement hors ligne.
