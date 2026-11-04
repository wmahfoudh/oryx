# Privacy

Oryx is a desktop program that runs on your machine. It has no account, no server of its own, and no telemetry: it does not report what you open, what you do, or that it is installed.

Oryx reaches the network in one case. When a markdown file or a book links to an image by URL (a badge on a README, for example), Oryx downloads that image so the page can show it. The downloaded images are kept in a cache folder on your machine so the file opens instantly the next time. `oryx --clear-cache` removes them.

The request goes to the address written in the file, through the system's proxy settings if there are any. It carries no cookie and nothing that names you or Oryx. The server of the image still sees your IP address and the time of the request, as with any download. A file written by someone else, with an image on a server of theirs, can tell that server when you opened the file. Oryx has no setting that turns remote images off.

Books, code and text files are read from disk. When you edit a file, it is written back to disk. Links you click open in your browser. Nothing leaves the machine.

What Oryx keeps on your machine:

- The settings, in `config.toml`: window size and position, the active theme, the sidebar state, the export preferences and the last folder opened.
- The reading positions of your books and comics, in `positions.toml`. For each of the last hundred, the file holds the identifier of the book (or its path on disk, when the book has none), the place where you stopped and the reading direction.
- The downloaded images, in the cache folder.
- The themes you edit or create, in the data folder.
- A copy of a new note (`Ctrl+M`) while you type it, in the notes folder, so the note can be recovered if Oryx closes without saving it. The copy is removed once the note is saved or discarded.
- Text you pipe into Oryx, such as `git diff | oryx`, as a file in the notes folder. The file is removed when you quit.

Where these folders are:

| System | Settings | Cache | Data | Notes |
|---|---|---|---|---|
| Linux | `~/.config/oryx` | `~/.cache/oryx` | `~/.local/share/oryx` | `~/.local/state/oryx/notes` |
| Windows | `%APPDATA%\oryx\config` | `%LOCALAPPDATA%\oryx\cache` | `%APPDATA%\oryx` | `%LOCALAPPDATA%\oryx\data\notes` |
| macOS | `~/Library/Application Support/oryx` | `~/Library/Caches/oryx` | `~/Library/Application Support/oryx` | `~/Library/Application Support/oryx/notes` |

The Windows row is for the MSI and the zip. With the Microsoft Store version, Windows keeps these files in a private folder of the app, and removes them when the app is uninstalled.

Deleting these folders removes what Oryx has kept. The file associations that an installer or `oryx --register` wrote are not in these folders, and they hold no personal data.

Besides the files and folders you open, Oryx reads the folders that the sidebar shows (your home folder at the first launch), the pictures that a file links to, and the fonts installed on the system.
