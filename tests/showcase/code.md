# This is Oryx rendering highlighted code

Fenced blocks sit in a bordered panel, one row per source line, with
keywords, strings, numbers, comments and operators each taking their color
from the active theme rather than from the language.

```rust
/// Opens a file and returns the parsed document.
pub fn open(path: &Path, deadline: Option<Instant>) -> Result<Opened> {
    let bytes = std::fs::read(path)?;
    if is_binary(&bytes) {
        bail!("{} is not a text file", path.display());
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut document = match detect(path) {
        FileKind::Markdown => markdown::parse(&text),
        FileKind::Code(token) => code_document(Some(token), &text),
        FileKind::Text => plain_document(&text),
        FileKind::Unknown => code_document(None, &text),
    };
    let pending = apply_budget(&mut document, deadline);
    Ok(Opened { document, pending })
}
```

```python
def summarize(rows, key="total"):
    """Group rows and total one column."""
    out = {}
    for row in rows:
        out[row.name] = out.get(row.name, 0) + row[key]
    return sorted(out.items(), key=lambda kv: -kv[1])
```

```bash
# Shell, with strings, comments and variables colored
for file in "$@"; do
    printf 'rendering %s\n' "$file"
    oryx --theme nord "$file" || echo "failed: $file" >&2
done
```

A line longer than the panel wraps inside it rather than spilling out or
being cut off:

```javascript
const themes = ["oryx-light", "oryx-dark", "dracula", "nord", "gruvbox-dark", "catppuccin-mocha", "tokyo-night", "solarized-light", "everforest-dark"];
```

Code with no language, or a language Oryx does not know, still gets the
panel and the monospace family, in the plain code color:

```
$ oryx README.md
opened in 41ms
```
