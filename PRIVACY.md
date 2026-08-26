# Privacy

Oryx is a desktop program that runs on your machine. It has no account, no server of its own, and no telemetry: it does not report what you open, what you do, or that it is installed.

Oryx reaches the network in one case. When a markdown file links to an image by URL (a badge on a README, for example), Oryx downloads that image so the page can show it. The request goes to the address written in the file, through the system's proxy settings if there are any, and carries nothing else. The downloaded images are kept in a cache folder on your machine so the file opens instantly the next time. `oryx --clear-cache` removes them.

Books, code and text files are read from disk. When you edit a file, it is written back to disk. Links you click open in your browser. Nothing leaves the machine.

What Oryx keeps on your machine:

- The settings, in `config.toml`: window size and position, the active theme, the sidebar state, the export preferences and the last folder opened.
- The reading positions of your books, in `positions.toml`.
- The downloaded images, in the cache folder.
- The themes you edit or create, in the data folder.

Where these folders are:

| System | Settings | Cache | Data |
|---|---|---|---|
| Linux | `~/.config/oryx` | `~/.cache/oryx` | `~/.local/share/oryx` |
| Windows | `%APPDATA%\oryx\config` | `%LOCALAPPDATA%\oryx\cache` | `%APPDATA%\oryx` |
| macOS | `~/Library/Application Support/oryx` | `~/Library/Caches/oryx` | `~/Library/Application Support/oryx` |

Inside a Flatpak, the same folders sit under `~/.var/app/com.steerania.Oryx`.

Deleting these folders removes everything Oryx has kept. Oryx does not read anything else on the machine besides the files and folders you open.
