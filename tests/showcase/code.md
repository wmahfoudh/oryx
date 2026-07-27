# This is Oryx rendering highlighted code

Fenced blocks display in a bordered panel, one row per source line, with
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

Raw text, Code with no language, or a language Oryx does not know, still gets the
panel and the monospace family:

```
$ oryx README.md
opened in 41ms
```
