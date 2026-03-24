# CEFR Highlight

A VSCode extension that highlights English words by their CEFR level (A1–C2) using semantic tokens, and shows word definitions on hover.

## Features

- **Semantic highlighting** — words are colored by CEFR level in plaintext and markdown files
- **Hover definitions** — hover over a word to see its CEFR level, part of speech, and topic

## Color Legend

| Level | Color | Description |
|-------|-------|-------------|
| A1 | Gray | Beginner |
| A2 | Amber | Elementary |
| B1 | Green | Intermediate |
| B2 | Blue | Upper-intermediate |
| C1 | Purple | Advanced |
| C2 | Red | Proficiency |

## Architecture

- **Server** (Rust / tower-lsp): loads a 7 243-word CEFR dictionary, tokenizes documents, returns semantic tokens and hover info via LSP
- **Client** (TypeScript): standard VSCode language-client extension that spawns the server binary over stdio

## Building

```bash
# Build the language server
cd server && cargo build --release

# Build the extension client
cd client && npm install && npm run compile
```

## Development

Press **F5** in VSCode to launch the Extension Development Host.
