# Magic Deck

Magic Deck est une application web locale pour consulter les decks, la collection et les wildcards de MTG Arena sous Ubuntu avec Steam/Proton.

Le serveur Rust lit `Player.log`, conserve le dernier état en mémoire et persiste la dernière collection connue sur disque. Il fournit :

- un tableau de bord local ;
- les decks et leur contenu détaillé ;
- une collection illustrée : recherche, couleurs, types, raretés, tri et pagination ;
- un mode galerie, des favoris et un aperçu agrandi au clic (fermeture par Échap ou clic autour) ;
- des exports Arena, JSON et CSV ;
- la configuration du chemin du journal depuis l'interface.

Le serveur écoute uniquement sur `127.0.0.1`. Lorsqu'une analyse IA est explicitement demandée, le deck, une présélection de 80 cartes possédées maximum et un contexte de règles borné sont envoyés à Google Gemini, pas l'inventaire complet. Les favoris restent dans le navigateur utilisé (`localStorage`) ; ils ne sont pas synchronisés entre navigateurs.

L'interface utilise [Motion 13.2.0](https://motion.dev/docs/quick-start) via CDN pour les transitions de vues, l'apparition progressive des decks et les retours d'action. Les animations sont désactivées ou réduites lorsque le navigateur signale une préférence de mouvement réduit.

Le détail d'un deck propose **Explorer mes combos** : synergies illustrées, séquences pas à pas, prérequis, bénéfices et limites, puis un petit défi à essayer en partie. Les onglets séparent combos, plan de jeu et pistes de construction. Les cartes et quantités sont validées contre la collection synchronisée ; les interactions de règles restent des conseils IA, pas des combos certifiés. Chaque rapport est daté, exportable en JSON et conservé avec les snapshots exacts du deck et de la collection. Les anciens rapports restent consultables.

Les miniatures de cartes sont chargées à la demande depuis les URLs d'image de [Scryfall](https://scryfall.com/docs/api/images), en privilégiant le couple set/numéro de collection puis le nom exact. En cas d'absence de réseau ou de correspondance, l'affichage textuel reste disponible.

## Prérequis

- Ubuntu ;
- Rust stable installé avec [rustup](https://rustup.rs/) ;
- les journaux détaillés activés dans MTGA : **Options → Account → Detailed Logs**.

Vérifier Rust :

```bash
rustc --version
cargo --version
```

## Démarrage rapide

Depuis la racine du projet :

```bash
cargo run --release
```

Ouvrir ensuite <http://127.0.0.1:8092>.

Le chemin MTGA utilisé par défaut est :

```text
~/.local/share/Steam/steamapps/compatdata/2141910/pfx/drive_c/users/steamuser/AppData/LocalLow/Wizards Of The Coast/MTGA/Player.log
```

Laisser MTGA ouvert, afficher au moins une fois les écrans **Collection** et **Decks**, puis cliquer sur **Refresh / Sync** dans Magic Deck. Les versions actuelles de MTGA ne placent plus les quantités possédées dans `Player.log` : sous Linux, Magic Deck les lit donc en lecture seule dans la mémoire du processus MTGA du même utilisateur.

Sous Windows, le même mécanisme utilise automatiquement les API natives `OpenProcess` / `ReadProcessMemory` pour lire la mémoire de `MTGA.exe`. Si la lecture est refusée ou si MTGA n'est pas ouvert, l'application réutilise la dernière collection sauvegardée et affiche un avertissement.

La collection est enregistrée dans `~/.local/share/magic-deck/collection.json` sous Linux (ou le dossier de données local équivalent sous Windows). Elle est mise à jour au démarrage et à chaque **Refresh / Sync** lorsqu'une source MTGA est disponible.

## Configuration

Le chemin peut être modifié dans l'écran **Settings** ou au démarrage :

```bash
MTGA_LOG_PATH=/chemin/vers/Player.log cargo run --release
```

Pour changer le port :

```bash
MAGIC_DECK_PORT=8092 cargo run --release
```

Avec le binaire déjà compilé :

```bash
MAGIC_DECK_PORT=8092 ./target/release/magic-deck
```

`PORT` est également accepté. La configuration enregistrée depuis l'interface se trouve dans `~/.config/magic-deck/config.json`.

Pour diagnostiquer un appel Gemini (prompt et réponse tronqués, sans la clé API), activer les logs détaillés :

```bash
RUST_LOG=magic_deck=info,magic_deck::ai_coach=debug MAGIC_DECK_PORT=8092 ./target/release/magic-deck
```

## Activer le coach IA

Créer une clé dans Google AI Studio, puis la saisir dans **Settings → Gemini API key**. Elle est stockée localement et masquée dans l'interface. `GEMINI_API_KEY` reste accepté au démarrage et est prioritaire :

```bash
export GEMINI_API_KEY='votre_nouvelle_cle'
cargo run --release
```

Le modèle configuré par défaut est `gemini-3.6-flash`, remplaçable avec `GEMINI_MODEL` selon les modèles accessibles à votre compte. La clé n'est jamais incluse dans les rapports ni renvoyée au navigateur. Les analyses réutilisent un rapport lorsque le deck, la collection, le modèle et la version du prompt correspondent. Les rapports sont stockés dans `~/.local/share/magic-deck/reports.sqlite3` ; `MAGIC_DECK_DB_PATH` permet de déplacer ce fichier.

Le prompt éditable est dans [`src/prompts/deck_coach.txt`](src/prompts/deck_coach.txt). Il distingue synergie, séquence, boucle répétable et boucle infinie, exige leurs coûts et conditions d'arrêt et autorise une réponse sans combo si les règles sont incertaines. Le contexte local des capacités est limité à 12 000 caractères ; la sortie reste plafonnée à 4 096 tokens. La présélection par couleurs/types n'est pas une recherche exhaustive de combos dans toute la collection.

Pour les decks `Standard` ou `Alchemy`, seules les extensions actuellement configurées comme légales sont proposées. La liste peut être ajustée après une rotation MTGA :

```bash
MAGIC_DECK_STANDARD_SETS='WOE,LCI,MKM,OTJ,BIG,BLB,DSK,FDN,FIN,EOE,TLA,ECL,SOA,MSH,TMT,HOB' ./target/release/magic-deck
```

Ce filtrage par extensions est une approximation, pas une garantie de légalité. La base locale MTGA ne contient pas les listes de bannissement par format ; les formats `Historic`, `Timeless`, `Explorer` et `Brawl` ne sont donc pas restreints par set.

La langue des noms de decks intégrés est détectée automatiquement. Elle peut être forcée ainsi :

```bash
MAGIC_DECK_LOCALE=frFR cargo run --release
```

## Compiler un binaire

```bash
cargo build --release
./target/release/magic-deck
```

## Validation

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

Test navigateur optionnel : lancer l'application sur le port 18092, puis utiliser une installation de Playwright avec Chromium :

```bash
PLAYWRIGHT_MODULE=/chemin/vers/node_modules/playwright node scripts/ui-smoke.cjs
```

Le script remplace les réponses API par des fixtures (aucun appel Gemini) et vérifie collection, filtres, favoris, aperçu au clic, rapports anciens/nouveaux sur bureau et mobile, avec mouvement réduit et sans le CDN Motion. `MAGIC_DECK_TEST_URL` remplace l'adresse de test ; `SCREENSHOT_DIR` active les captures. Aucun build frontend n'est requis : après une modification des fichiers `web/` embarqués, relancer `cargo run --release`.

## Limite des métadonnées MTGA

MTGA ne journalise généralement que les identifiants numériques des cartes. Magic Deck les résout automatiquement avec la base locale `Raw_CardDatabase_*.mtga` installée par MTGA. Si celle-ci se trouve dans une autre bibliothèque Steam non détectée :

```bash
MTGA_CARD_DB_PATH="/chemin/vers/Raw_CardDatabase_xxx.mtga" cargo run --release
```

Les choix techniques, flux et pistes d'évolution sont détaillés dans [ARCHITECTURE.md](ARCHITECTURE.md).
