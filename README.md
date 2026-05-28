# Reader

`reader` is a local-first command line reading tool for EPUB and TXT books.

## What works in v1

- Open a local book directly: `reader book.epub`
- Add/list/open/remove books in a local bookshelf
- Resume the most recently read book
- Full-screen terminal reading UI with progress saving
- EPUB/TXT parsing
- Table of contents navigation
- Metadata view
- Display configuration
- Bookmarks
- Book search

Reader stores its state in the user's config directory and never writes metadata
next to book files.

## Commands

```powershell
reader <file>
reader add <file>
reader list
reader open <book-id-or-title>
reader resume
reader remove <book-id-or-title>
reader bookmark add <book-id-or-title> <label>
reader bookmark list <book-id-or-title>
reader bookmark goto <book-id-or-title> <bookmark-id>
reader bookmark delete <book-id-or-title> <bookmark-id>
reader search <book-id-or-title> <query>
```

## Reader Keys

Reader has one root reading mode and a few child panels. `Esc` returns from a
child panel to reading mode. At the reading root, `Esc` quits and saves progress.
`Ctrl+C` quits and saves from any mode.

| Mode | Key | Action |
| --- | --- | --- |
| Reading | `q` / `Esc` | Quit and save progress |
| Reading | `j` / `Down` / `PageDown` / `Space` | Next page |
| Reading | `k` / `Up` / `PageUp` | Previous page |
| Reading | `g` / `G` | First / last page |
| Reading | `t` | Open table of contents |
| Reading | `M` | Open book metadata |
| Reading | `/` | Open search prompt |
| Reading | `c` | Switch color theme |
| Reading | `s` | Show or hide progress |
| Reading | `b` | Add a bookmark at the current page |
| Reading | `n` / `N` | Next / previous search result |
| Table of contents | `j/k` or arrows | Move selection |
| Table of contents | `g` / `G` | First / last entry |
| Table of contents | `Enter` | Jump to selected section and return |
| Table of contents | `Esc` / `q` / `t` | Return without jumping |
| Metadata | `Esc` / `Enter` / `q` / `M` | Close and return |
| Search | text | Type query |
| Search | `Backspace` | Delete character |
| Search | `Enter` | Run search and return |
| Search | `Esc` | Cancel and return |

## Configuration

Reader stores display settings in:

- Linux and macOS: `~/.config/reader/configuration.json`
- Windows: `%APPDATA%\reader\configuration.json`

Supported settings:

```json
{
  "theme": "dark",
  "show_progress": true,
  "mouse_support": false,
  "text_width": null
}
```

`theme` can be `dark` or `light`. `text_width` may be set to a terminal
column width, or left as `null` to use the available terminal width.

## Not Yet Supported

Reader intentionally keeps this stage focused on core EPUB/TXT reading. FB2,
MOBI, AZW3, URL reading, dictionary lookup, text-to-speech, double spread, and
seamless between-chapter reading are not implemented yet.
