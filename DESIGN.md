---
name: Magic Deck
description: A focused local command center for MTGA decks, collection and coaching.
colors:
  ink: "#0b0d10"
  surface: "#12161b"
  panel: "#171d23"
  slate: "#27313a"
  line: "#34404a"
  chalk: "#f2eee5"
  gold: "#d9ae61"
  rust: "#b9543d"
typography:
  display:
    fontFamily: "Barlow Condensed, IBM Plex Sans, sans-serif"
    fontSize: "3rem"
    fontWeight: 700
    lineHeight: 1
  body:
    fontFamily: "IBM Plex Sans, sans-serif"
    fontSize: "0.875rem"
    fontWeight: 400
    lineHeight: 1.5
  label:
    fontFamily: "DM Mono, monospace"
    fontSize: "0.6875rem"
    letterSpacing: "0.12em"
rounded:
  sm: "8px"
  md: "12px"
spacing:
  sm: "8px"
  md: "16px"
  lg: "32px"
components:
  button-primary:
    backgroundColor: "{colors.gold}"
    textColor: "{colors.ink}"
    rounded: "{rounded.sm}"
    padding: "12px 16px"
  button-secondary:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.chalk}"
    rounded: "{rounded.sm}"
    padding: "12px 16px"
---

# Design System: Magic Deck

## Overview

**Creative North Star: “The Collector’s Strategy Workshop”**

Magic Deck is a local workshop for enjoying a player's MTGA library. Preserve its mineral ink surfaces, chalk text and rare brass action signals, but let card artwork and strategic exploration lead. Quieter framing, softer corners and progressive disclosure make room for curiosity without turning the app into a neon gaming dashboard.

Data is grouped into clear work surfaces, with a single strong action per context. Collection browsing favors artwork; deck inspection keeps precise quantities and strategic information accessible. Rarity is metadata, not a claim of power.

## Colors

The palette is dark and mineral, with brass reserved for focus and primary action.

### Primary
- **Signal Brass** (#d9ae61): sync, export, selected navigation and key metrics.

### Secondary
- **Kiln Rust** (#b9543d): warnings and destructive/error emphasis.

### Neutral
- **Deep Ink** (#0b0d10): application canvas.
- **Slate Surface** (#12161b): secondary work surfaces.
- **Plate Panel** (#171d23): content containers.
- **Chalk** (#f2eee5): primary text.
- **Structural Line** (#34404a): borders and separators.

### Named Rules
**The Brass Scarcity Rule.** Brass is a signal, not wallpaper: use it for action, selection and meaningful state only.

## Typography

**Display Font:** Barlow Condensed (with IBM Plex Sans fallback)

**Body Font:** IBM Plex Sans

**Label/Mono Font:** DM Mono

Headings are condensed and confident; body copy is neutral and readable; mono labels make paths, counts and timestamps feel inspectable.

## Layout

The desktop shell uses a fixed 16rem index rail and a centered content field. Screens favor split work areas, dense lists and a few high-value metrics over repeated equal cards. At mobile widths the rail becomes a compact top navigation and content collapses to one column.

## Elevation & Depth

Depth uses one-pixel lines and tonal changes, without offset shadows or decorative corner planes. Reserve dimming for the focused card dialog. Motion explains opening/closing and step changes, never delays reading a heading; reduced motion and a missing animation CDN must remain fully usable.

## Shapes

Controls use 8px corners and larger surfaces 12px. Focus rings remain brass and visible. Card illustrations retain their own proportions without cropping rules text in the enlarged viewer.

## Components

- **Navigation:** concise French labels; active state uses brass text on a tonal surface.
- **Primary button:** calm brass surface with clear keyboard and hover feedback.
- **Secondary button:** slate plate with a structural border.
- **Deck tile:** clear title, format, colors and available win rate without decorative planes.
- **Collection:** 36-card pages, gallery mode and local favorite hearts; advanced filters collapse on mobile.
- **Combo workshop:** illustrated pieces, prerequisites, numbered steps, payoff and disclosed limitations; separate plan and improvement sections.
- **Data row:** compact, divided rows with mono metadata and optional card image.
- **Status:** semantic colors for success, warning, error and loading; loading uses skeletons where possible.

## Do's and Don'ts

- Do keep the main action obvious and the data legible.
- Do give artwork breathing room and explain strategic interactions without overstating certainty.
- Do preserve keyboard focus, responsive behavior and meaningful empty states.
- Don't use gradients, glass panels, oversized rounded cards or decorative icon glyphs.
- Don't use brass on every label; preserve its signal value.
