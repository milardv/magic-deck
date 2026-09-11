# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Stack

delegated: Rust/Axum avec une interface HTML embarquée et Tailwind CSS via CDN.

## Users

Joueurs de Magic: The Gathering Arena qui veulent consulter localement leurs decks, leur collection et leurs performances sans compte cloud.

Motivation exprimée : prendre plaisir à admirer les illustrations, collectionner ses cartes préférées et réfléchir aux stratégies, synergies et constructions originales. La puissance compétitive est un intérêt, pas l'unique objectif.

## Product Purpose

Magic Deck transforme les données locales de MTGA (`Player.log` et la base de cartes) en une bibliothèque lisible de decks et de cartes, avec exports et coaching IA optionnel.

## Positioning

Un compagnon local qui conserve les données de collection et de decks sur la machine de l'utilisateur, tout en permettant une analyse Gemini explicitement déclenchée.

## Operating Context

L'utilisateur joue à MTGA via Steam/Proton ou Windows, ouvre l'application locale dans son navigateur, synchronise après avoir ouvert les écrans Decks/Collection, puis inspecte ou exporte ses données.

## Capabilities and Constraints

- Dashboard, liste et détail des decks, collection filtrable, exports Arena/JSON/CSV et Settings.
- Filtre des decks créés par l'utilisateur, statistiques disponibles, courbe de mana et miniatures Scryfall à la demande.
- Analyse Gemini optionnelle, validée contre la collection et conservée avec son historique.
- Galerie avec favoris locaux et aperçu au clic ; atelier de combos illustré, étapes interactives et défi de jeu. Aucune combinaison n'est présentée comme certifiée par un moteur de règles.
- Le serveur écoute localement sur 127.0.0.1; le port est configurable.
- L'interface doit rester utilisable sans réseau; les images Scryfall sont donc facultatives.

## Brand Commitments

Nom : Magic Deck. L'identité doit évoquer un outil de bibliothèque de cartes fiable et premium, sans reproduire d'assets propriétaires de Wizards.

## Evidence on Hand

- Interface embarquée dans `web/index.html`, `app.css`, `app.js`, `collection.js`, `coach.js` et `preview.js`.
- Routes Axum et parser Rust dans `src/`.
- Données réelles issues de `Player.log` et de la base MTGA locale.
- Aucune illustration ou police de marque fournie par l'utilisateur.

## Product Principles

- Local par défaut et transparent sur les données envoyées.
- Lire et comparer rapidement avant d'explorer.
- Montrer les informations utiles au moment de la décision.
- Tolérer les données MTGA incomplètes sans bloquer l'utilisateur.

## Accessibility & Inclusion

Contrastes lisibles, navigation clavier, états de chargement/erreur/absence de données explicites et composition utilisable sur mobile.
