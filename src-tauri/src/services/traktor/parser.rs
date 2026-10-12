//! Streaming parser for Traktor `collection.nml` documents.

use std::path::Path;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use super::model::{NmlCue, NmlDocument, NmlNodeKind, NmlPlaylistNode, NmlTrack};
use super::Result;

/// Parse an NML document from an in-memory string.
pub fn parse_str(xml: &str) -> Result<NmlDocument> {
    parse_events(xml)
}

/// Read and parse an NML document from disk. The caller owns the path; the
/// parser never writes and never inspects anything but the file itself.
pub fn parse_file(path: impl AsRef<Path>) -> Result<NmlDocument> {
    let xml = std::fs::read_to_string(path)?;
    parse_str(&xml)
}

/// Read the first occurrence of `name` from an element's attributes, decoded.
fn attr(element: &BytesStart<'_>, name: &[u8]) -> quick_xml::Result<Option<String>> {
    for attribute in element.attributes() {
        let attribute = attribute?;
        if attribute.key.as_ref() == name {
            return Ok(Some(
                attribute
                    .normalized_value(quick_xml::XmlVersion::Explicit1_0)?
                    .into_owned(),
            ));
        }
    }
    Ok(None)
}

fn parse_f64(element: &BytesStart<'_>, name: &[u8]) -> quick_xml::Result<Option<f64>> {
    Ok(attr(element, name)?.and_then(|value| value.trim().parse().ok()))
}

fn parse_i32(element: &BytesStart<'_>, name: &[u8]) -> quick_xml::Result<Option<i32>> {
    Ok(attr(element, name)?.and_then(|value| value.trim().parse().ok()))
}

fn parse_events(xml: &str) -> Result<NmlDocument> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut document = NmlDocument::default();
    let mut current_track: Option<NmlTrack> = None;
    let mut current_cue: Option<NmlCue> = None;
    let mut playlist_stack: Vec<NmlPlaylistNode> = Vec::new();
    let mut in_collection = false;

    loop {
        let event = reader.read_event()?;
        match &event {
            Event::Start(element) | Event::Empty(element) => {
                let is_empty = matches!(&event, Event::Empty(_));
                match element.name().as_ref() {
                    b"NML" => document.version = attr(element, b"VERSION")?,
                    b"COLLECTION" => in_collection = true,
                    b"ENTRY" => {
                        if in_collection {
                            current_track = Some(NmlTrack {
                                audio_id: attr(element, b"AUDIO_ID")?,
                                title: attr(element, b"TITLE")?,
                                artist: attr(element, b"ARTIST")?,
                                ..Default::default()
                            });
                        }
                    }
                    b"LOCATION" => {
                        if let Some(track) = current_track.as_mut() {
                            track.location.volume = attr(element, b"VOLUME")?;
                            track.location.dir = attr(element, b"DIR")?.unwrap_or_default();
                            track.location.file = attr(element, b"FILE")?.unwrap_or_default();
                        }
                    }
                    b"ALBUM" => {
                        if let Some(track) = current_track.as_mut() {
                            track.album = attr(element, b"TITLE")?;
                        }
                    }
                    b"INFO" => {
                        if let Some(track) = current_track.as_mut() {
                            track.info_key = attr(element, b"KEY")?;
                        }
                    }
                    b"TEMPO" => {
                        if let Some(track) = current_track.as_mut() {
                            track.tempo_bpm = parse_f64(element, b"BPM")?;
                        }
                    }
                    b"MUSICAL_KEY" => {
                        if let Some(track) = current_track.as_mut() {
                            track.musical_key = parse_i32(element, b"VALUE")?;
                        }
                    }
                    b"CUE_V2" => {
                        if let Some(track) = current_track.as_mut() {
                            let cue = NmlCue {
                                name: attr(element, b"NAME")?,
                                traktor_type: parse_i32(element, b"TYPE")?.unwrap_or(0),
                                start_ms: parse_f64(element, b"START")?.unwrap_or(0.0),
                                length_ms: parse_f64(element, b"LEN")?.unwrap_or(0.0),
                                hot_cue: parse_i32(element, b"HOTCUE")?.unwrap_or(-1),
                                grid_bpm: None,
                            };
                            // A grid cue is a container for `<GRID>`; a plain cue is self-closing.
                            if is_empty {
                                track.cues.push(cue);
                            } else {
                                current_cue = Some(cue);
                            }
                        }
                    }
                    b"GRID" => {
                        if let Some(cue) = current_cue.as_mut() {
                            cue.grid_bpm = parse_f64(element, b"BPM")?;
                        }
                    }
                    b"NODE" => {
                        let kind = match attr(element, b"TYPE")?.as_deref() {
                            Some("FOLDER") => NmlNodeKind::Folder,
                            Some("SMARTLIST") => NmlNodeKind::Smartlist,
                            _ => NmlNodeKind::Playlist,
                        };
                        playlist_stack.push(NmlPlaylistNode {
                            name: attr(element, b"NAME")?.unwrap_or_default(),
                            kind,
                            ..Default::default()
                        });
                    }
                    b"PLAYLIST" | b"SMARTLIST" => {
                        if let Some(node) = playlist_stack.last_mut() {
                            if node.uuid.is_none() {
                                node.uuid = attr(element, b"UUID")?;
                            }
                        }
                    }
                    b"SEARCH_EXPRESSION" => {
                        if let Some(node) = playlist_stack.last_mut() {
                            node.search_query = attr(element, b"QUERY")?;
                        }
                    }
                    b"PRIMARYKEY" => {
                        if let Some(node) = playlist_stack.last_mut() {
                            if let Some(key) = attr(element, b"KEY")? {
                                node.track_keys.push(key);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Event::End(element) => match element.name().as_ref() {
                b"COLLECTION" => in_collection = false,
                b"ENTRY" => {
                    if let Some(track) = current_track.take() {
                        document.tracks.push(track);
                    }
                }
                b"CUE_V2" => {
                    if let Some(cue) = current_cue.take() {
                        if let Some(track) = current_track.as_mut() {
                            track.cues.push(cue);
                        }
                    }
                }
                b"NODE" => {
                    if let Some(node) = playlist_stack.pop() {
                        attach_node(&mut document, &mut playlist_stack, node);
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(document)
}

/// The `$ROOT` node is Traktor's virtual root; its children are the real forest.
fn attach_node(document: &mut NmlDocument, stack: &mut [NmlPlaylistNode], node: NmlPlaylistNode) {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else if node.name == "$ROOT" && node.kind == NmlNodeKind::Folder {
        document.playlists.extend(node.children);
    } else {
        document.playlists.push(node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_xml_is_an_error() {
        // Mismatched end tag: well-formedness checking must reject it.
        assert!(parse_str("<NML></COLLECTION>").is_err());
    }
}
