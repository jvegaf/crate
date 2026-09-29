---
title: Library Management
description: Import, organize, and manage your track collection
---

Your library is the central hub for all your audio files. Crate automatically extracts metadata and keeps track of your collection.

## Importing Tracks

### Drag and Drop

The easiest way to add tracks is to drag audio files or folders directly onto the Crate window.

1. Open your file manager
2. Select files or folders containing audio
3. Drag them onto the Crate window
4. Crate will process and import the files

### Import Dialog

For more control, use the import dialog:

1. Click **Import** in the toolbar
2. Select files or folders in the dialog
3. Click **Open** to begin the import

### Supported Formats

Crate supports common audio formats:

| Format | Extensions |
|--------|------------|
| MP3 | `.mp3` |
| FLAC | `.flac` |
| WAV | `.wav` |
| AIFF | `.aiff`, `.aif` |
| AAC | `.m4a`, `.aac` |

## Automatic Metadata Extraction

When you import tracks, Crate automatically extracts:

- **Title** - Track name
- **Artist** - Performer
- **Album** - Album name
- **Year** - Release year
- **Genre** - Music genre
- **Label** - Record label
- **Catalog Number** - Label catalog ID
- **BPM** - Beats per minute
- **Key** - Musical key
- **Duration** - Track length
- **Bitrate** - Audio quality
- **Sample Rate** - Audio sample rate
- **Album Artwork** - Embedded cover art

Metadata is read from embedded ID3 tags (MP3) or equivalent metadata containers for other formats.

## Handling Duplicates

When importing a track that already exists in your library (detected by file hash), Crate offers options:

| Option | Effect |
|--------|--------|
| **Skip** | Keep the existing track, ignore the new file |
| **Update Path** | Keep existing metadata, update the file path |
| **Replace** | Replace the existing track with the new file |

This prevents accidental duplicates while giving you control over updates.

## Missing Tracks

If a file is moved or deleted outside of Crate, the track becomes "missing." Crate tracks this so you can:

- See which tracks are missing
- Relocate tracks to their new location
- Remove orphaned entries from your library

### Relocating a Missing Track

1. Right-click the missing track
2. Select **Relocate**
3. Navigate to the file's new location
4. Select the file and click **Open**

The track's metadata is preserved; only the file path is updated.

## File Integrity

Crate uses BLAKE3 hashing to verify file integrity. Each track's hash is stored when imported, allowing Crate to:

- Detect duplicate files accurately
- Identify when files have been modified
- Verify file integrity over time

## Track Properties

Each track in your library has properties you can view and edit:

### Customizing the Tracklist Columns

The tracklist shows the columns you need, in the order you want them. Your layout is saved with your other preferences, so it is still there the next time you open Crate. It applies to the library and to every playlist.

**Choose which columns are visible.** Right-click anywhere on the column header row. The menu lists every column, with a check mark next to the ones currently shown. Click an entry to show or hide it — the menu stays open so you can adjust several columns in a row. Title cannot be hidden: a tracklist without titles is not readable, so that entry is greyed out. Choose **Reset to default columns** at the bottom of the menu to go back to the original layout.

| Column | What it shows | Shown by default |
|--------|---------------|------------------|
| Color | Your track color marker | Yes |
| Artwork | Album art thumbnail | Yes |
| Title | Track name (falls back to the file name) | Yes, always |
| Artist | Performer | Yes |
| BPM | Tempo | Yes |
| Key | Musical key, in your chosen notation | Yes |
| Time | Duration | Yes |
| Tags | Assigned tags, up to three plus a count | Yes |
| Rating | Star rating | Yes |
| Album | Album name | No |
| Label | Record label | No |
| Origin | Name of the folder that holds the file | No |
| Bitrate | Audio quality, in kbps | No |
| Year | Release year | No |
| Date Added | When the track entered your library | No |

**Change the order.** Drag a column header left or right. While you drag, a thin vertical marker shows where the column will land: drop on the left half of a header to place the column before it, on the right half to place it after. The other columns shift to make room.

**Widths adjust automatically.** You cannot set a width, and you do not need to: the table always fills the space it has. Compact columns (color, artwork, duration, BPM, key, bitrate, year, date added, rating) keep a fixed width, while text columns (title, artist, album, label, origin, tags) share whatever space is left. Hiding a wide text column therefore gives that room back to the others instead of leaving a gap on the right.

The Origin column is the name of the folder the audio file sits in, not the whole path — useful when you organised your collection in folders before importing it. Hover the cell to see the full name if it is cut off.

### Viewable Properties

| Property | Description |
|----------|-------------|
| Title | Track name |
| Artist | Performer |
| Album | Album name |
| Duration | Length (MM:SS) |
| BPM | Tempo |
| Key | Musical key |
| Genre | Music style |
| Year | Release year |
| Label | Record label |
| Catalog # | Label catalog number |
| Bitrate | Audio quality (kbps) |
| Sample Rate | Audio sample rate (Hz) |
| Format | File format |
| Rating | Star rating |
| Play Count | Times played |
| Date Added | When imported |
| Last Played | Most recent play |

### Editing a Single Track's Metadata

For a focused view of one track, right-click it in the library and choose **Edit Metadata**. The modal shows editable metadata alongside technical and library details.

Editable audio metadata:

- Title, Artist, Album, Year, Genre, Label, Catalog Number
- BPM and Key
- Embedded artwork

Rating is stored in Crate's library rather than embedded audio tags. It ranges from 0 to 5; clearing the rating sets it to 0, meaning unrated.

Use **Clear Field** to remove nullable text or numeric metadata explicitly. Leaving a field unchanged does not send it to the audio-file writer.

The modal also lets you edit Crate tags and Crate-managed artwork. These are library data, separate from embedded audio metadata and embedded artwork. For tag categories and organization, see [Tagging](/crate/user-guide/tagging/).

Duration, format, bitrate, sample rate, play count, dates, file path, and other technical/library properties are read-only in this modal. The Inspector remains available for its existing quick and bulk editing workflows, including color and tags.

#### Saving embedded metadata

Crate stages embedded tag and artwork changes before replacing the audio file. If the file format cannot write a requested embedded change, or the staged write fails, that metadata update is rejected and an error is shown. SQLite and the audio file cannot be made crash-atomic together, so a sudden process or system failure during replacement may still require recovery. Crate tags and Crate artwork use separate operations; the modal reports partial failures if one of those operations succeeds while another fails.

## Track Colors

Assign colors to tracks for visual organization:

- **Pink**
- **Red**
- **Orange**
- **Yellow**
- **Green**
- **Aqua**
- **Blue**
- **Purple**

To set a color:
1. Select one or more tracks
2. Right-click and choose **Set Color**
3. Select a color

Colors appear as a dot in the track list for quick identification.

## Removing Tracks

To remove tracks from your library:

1. Select the tracks to remove
2. Right-click and choose **Remove from Library**
3. Confirm the action

This removes the tracks from Crate's database but does not delete the actual files from disk.
