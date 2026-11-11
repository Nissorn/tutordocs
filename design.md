# Design: TutorDocs

The locked design system for the app, left by the Hallmark pass in ticket 68. Every screen reads the same tokens from `ui/src/styles.css`. Add a value there before using it in a component sheet. Do not type a raw colour, font or size into a component.

Hallmark verbs run on this app were `audit` and `redesign`, inside the existing code. The audit report is not published.

## Genre

Editorial reading. A learner spends the session in the chapter column, so the page is paper and type first, and the chrome around it stays quiet.

## Theme

Custom theme "Fountain Pen": warm paper, iron-gall ink, a blue-black accent. It is custom because Hallmark's named catalog is built for marketing pages and none of its themes is a quiet reading surface with an accent that passes AA as small text.

Axes for the diversification rule: paper band light, display style roman serif, accent hue cool (255 degrees).

### Palette

All colour is OKLCH, tinted toward the paper hue. Light and dark share the hue. Only lightness and chroma move.

| Token | Light | Dark | Use |
|---|---|---|---|
| `--paper` | `oklch(98.2% 0.006 85)` | `oklch(20% 0.01 70)` | the reading surface and every panel |
| `--sidebar-bg` | `oklch(95.8% 0.008 85)` | `oklch(17% 0.009 70)` | chrome that recedes: sidebar, tab bar |
| `--ink` | `oklch(24% 0.012 65)` | `oklch(92% 0.01 85)` | body text |
| `--ink-muted` | `oklch(44% 0.014 70)` | `oklch(74% 0.012 80)` | secondary text |
| `--ink-quiet` | `oklch(50% 0.014 70)` | `oklch(66% 0.012 80)` | helper text, placeholders, empty states |
| `--ink-faint` | `oklch(62% 0.012 75)` | `oklch(52% 0.01 75)` | marks and borders only, never text |
| `--line` | `oklch(88% 0.01 85)` | `oklch(32% 0.01 70)` | hairlines |
| `--accent` | `oklch(42% 0.14 255)` | `oklch(76% 0.11 255)` | links, the one rare colour |
| `--accent-soft` / `--accent-ink` | tinted ground and its ink | same, inverted | the current chapter in the sidebar |
| `--focus` | `oklch(52% 0.17 255)` | `oklch(72% 0.14 255)` | keyboard focus ring |

The accent covers about 3% of a screen. Status tints (`--busy-*`, `--ok-*`, `--warn-ink`, `--danger`, `--bad-bg`) and the five highlight colours keep their meanings and are tuned to the same paper.

`scripts/check-contrast.py` runs in `scripts/check.sh`. It fails the gate when any text pair drops below WCAG AA (4.5:1 for text, 3:1 for the focus ring and control borders) in any of the four theme blocks.

## Typography

Three families, all under the SIL Open Font License and bundled as files in `ui/src/fonts/`, because the app makes no network request for a font.

- Reading serif: Newsreader, with an optical-size axis. The chapter column, every page title, and the quiz prompts. Settings default to Serif.
- Reading sans: Atkinson Hyperlegible Next. What "Sans" in Settings selects.
- Code: JetBrains Mono.
- Chrome: the system sans, so menus and the app bar look native.

Scripts the bundled faces do not cover, Thai included, fall through to the system font in each stack.

The reading column is built on the three variables Settings drives: `--text-scale`, `--reading-font`, `--line-width`. Base size is 1.2rem times the scale, leading 1.7, body weight 400 in light and 380 in dark. At Medium and Normal width a line holds about 70 characters. Headings are roman, never italic, and carry hierarchy with size and weight. A quote is a tinted callout with no stripe down the side. Figures get a hairline frame and a measured caption. Tables are set in the system sans at 0.8em with tabular numerals. Links use the accent with a thin underline.

## Spacing, radius, controls

A 4pt scale: `--space-3xs` 2px, `--space-2xs` 4px, `--space-xs` 8px, `--space-sm` 12px, `--space-md` 16px, `--space-lg` 24px, `--space-xl` 40px, `--space-2xl` 64px.

Radii: `--radius-sm` 4px, `--radius-md` 6px, `--radius-lg` 8px, `--radius-xl` 12px, `--radius-pill`.

Type for chrome: `--text-2xs` 11px, `--text-xs` 12px, `--text-sm` 13px, `--text-md` 14px, `--text-lg` 16px.

A text button is 32px tall (`:where(.quiet-button, .outline-submit)` in `generate.css`). A compact control is 28px (`--control-h-sm`). The base button has no specificity, so a variant in another stylesheet wins by its own class and never by load order. Any control that can be clicked is at least 28px in its short side.

## Motion

Almost none. A 120ms chevron turn on the study panel headers and a pulse on the busy dot. `prefers-reduced-motion` stops the pulse. Easing is the named `--ease-out`, never the browser default.

## What every screen shares

- The tokens above, read from `styles.css`.
- One group-label look and one left inset in the sidebar.
- One focus ring: 2px `--focus`, visible on every keyboard-focusable control.
- Sentence-case labels. An uppercase label is kept only where it names a real group.

## What a screen may vary

Layout inside its own column. The chapter column, the outline review, the quiz, the PDF reader and the Home grid already differ and should.

## Where the pieces live

- Tokens and the reading surface: `ui/src/styles.css`.
- Fonts: `ui/src/fonts.css` and `ui/src/fonts/`.
- Settings that drive the reading column: `ui/src/settings.ts`.
- One owner per selector and property: `scripts/check-css-owners.py`.
