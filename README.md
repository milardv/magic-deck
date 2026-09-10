# Magic Deck

Magic Deck est une application web locale pour consulter les decks, la collection et les wildcards de MTG Arena sous Ubuntu avec Steam/Proton.

Le serveur Rust lit `Player.log`, conserve le dernier état en mémoire et fournit :

- un tableau de bord local ;
- les decks et leur contenu détaillé ;
- une collection recherchable et filtrable ;
- des exports Arena, JSON et CSV ;
- la configuration du chemin du journal depuis l'interface.

Le serveur écoute uniquement sur `127.0.0.1`. Les données restent locales, sauf lorsqu'une analyse IA est explicitement demandée : le deck et la collection courante sont alors envoyés à l'API Google Gemini.

Le détail d'un deck permet également de demander une analyse stratégique à Gemini. Les propositions sont validées contre la collection synchronisée et chaque rapport est conservé avec les snapshots exacts du deck et de la collection.

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

Créer une clé dans Google AI Studio, puis démarrer avec une variable d'environnement :

```bash
export GEMINI_API_KEY='votre_nouvelle_cle'
cargo run --release
```

Le modèle stable par défaut est `gemini-3.6-flash`. Il peut être remplacé avec `GEMINI_MODEL`. La clé n'est ni enregistrée ni renvoyée au navigateur. Les analyses réutilisent automatiquement un rapport existant lorsque le deck et la collection n'ont pas changé. Les rapports sont stockés dans `~/.local/share/magic-deck/reports.sqlite3` ; `MAGIC_DECK_DB_PATH` permet de déplacer ce fichier.

Pour les decks `Standard` ou `Alchemy`, seules les extensions actuellement configurées comme légales sont proposées. La liste peut être ajustée après une rotation MTGA :

```bash
MAGIC_DECK_STANDARD_SETS='WOE,LCI,MKM,OTJ,BIG,BLB,DSK,FDN,FIN,EOE,TLA,ECL,SOA,MSH,TMT,HOB' ./target/release/magic-deck
```

La base locale MTGA ne contient pas les listes de bannissement par format ; les formats `Historic`, `Timeless`, `Explorer` et `Brawl` ne sont donc pas restreints par set.

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

## Limite des métadonnées MTGA

MTGA ne journalise généralement que les identifiants numériques des cartes. Magic Deck les résout automatiquement avec la base locale `Raw_CardDatabase_*.mtga` installée par MTGA. Si celle-ci se trouve dans une autre bibliothèque Steam non détectée :

```bash
MTGA_CARD_DB_PATH="/chemin/vers/Raw_CardDatabase_xxx.mtga" cargo run --release
```

Les choix techniques, flux et pistes d'évolution sont détaillés dans [ARCHITECTURE.md](ARCHITECTURE.md).
