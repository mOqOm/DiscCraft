English | [日本語](SECURITY.ja.md)

# Security policy

DiscCraft reads files it did not make: video, sound, subtitles, pictures, Photoshop and Craft documents, DiscCraft
projects, and DVD and Blu-ray discs, folders and images. It writes disc images and drives disc burners, and it can
be driven from the command line (`disccraft-cli`) and through an MCP server (`disccraft-cli mcp`). It also checks
GitHub for a newer release and can download and install it (**Help ▸ Check for Updates…**). Reports about any of
these are welcome.

## Supported versions

DiscCraft is in alpha (0.1.0-alpha). Only the latest release on
[GitHub's releases](https://github.com/mOqOm/DiscCraft/releases) is supported; fixes go into the next release.

## Reporting a vulnerability

Please report privately through GitHub: on the repository's **Security** tab, choose **Report a vulnerability**
([private vulnerability reporting](https://github.com/mOqOm/DiscCraft/security/advisories/new)). Do not open a public
issue, and do not post a working exploit, a malicious file or a private project in public.

A useful report has:

- the DiscCraft version (Help ▸ About DiscCraft) and the system (Windows, macOS or Linux, and the processor),
- what is affected and what an attacker could do,
- the steps to reproduce it,
- for a file that DiscCraft mishandles: its format, its size and a SHA-256 hash, and whether it is synthetic or
  someone's own material. Share the file itself only when asked, through the private report,
- the logs that **Help ▸ Reveal Log Files…** shows (with the crash log, if DiscCraft ended unexpectedly), with
  personal paths and unrelated project content removed.

DiscCraft is maintained by one person. Reports are answered as soon as possible, but there is no fixed deadline.
Please allow time for a fix before disclosing the details.

## What counts

- DiscCraft is meant never to crash or hang on any input: a file, disc or project that makes it panic, freeze, or use
  memory or disk out of all proportion is a bug worth reporting, privately when it could be used against someone.
- Reading or writing outside the folders the user chose, running something DiscCraft did not mean to run, or an
  update that installs something other than DiscCraft's own release.
- A problem in a library DiscCraft uses (craft-libs, FilmCraft, PhotoCraft, EffectCraft, SoundCraft and others) is
  welcome here too; it is passed on to that project.

Not covered:

- Copy protection (CSS, AACS): DiscCraft does not read or write protected discs.
- The programs are not signed with a developer certificate yet, so Windows SmartScreen and macOS Gatekeeper ask
  before the first start. This is known.
- Questions about using DiscCraft and feature requests: please use [the issues](https://github.com/mOqOm/DiscCraft/issues).
