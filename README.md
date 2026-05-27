# Reader

`reader` is a local-first command line reading tool for EPUB and TXT books.

## What works in v1

- Open a local book directly: `reader book.epub`
- Add/list/open/remove books in a local bookshelf
- Resume the most recently read book
- Full-screen terminal reading UI with progress saving
- EPUB/TXT parsing
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

- `q` / `Esc`: quit and save progress
- `j` / `Down` / `PageDown` / `Space`: next page
- `k` / `Up` / `PageUp`: previous page
- `g`: first page
- `G`: last page
- `b`: add a bookmark at the current page
- `/`: search prompt
- `n`: next search result
- `N`: previous search result
